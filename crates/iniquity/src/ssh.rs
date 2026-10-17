//! SSH transport. Authentication is left to the BBS's own logon screens,
//! as with a telnet or modem caller, so every SSH auth attempt is accepted
//! and the SSH user name is only used as a suggested handle.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use russh::keys::ssh_key::LineEnding;
use russh::keys::ssh_key::private::RsaKeypair;
use russh::keys::{Algorithm, EcdsaCurve, PrivateKey};
use russh::{Preferred, cipher, kex, mac};
use russh::server::{Auth, ChannelOpenHandle, Msg, Server as _, Session};
use russh::{Channel, ChannelId, Pty};
use tokio::sync::watch;

use crate::BbsState;
use crate::session::{self, Caller};

pub async fn serve(state: Arc<BbsState>) -> anyhow::Result<()> {
    let dir = state.config().resolve(&state.config().ssh.host_key_dir);
    let keys = [
        ("ssh_host_ed25519_key", Algorithm::Ed25519),
        ("ssh_host_ecdsa_key", Algorithm::Ecdsa { curve: EcdsaCurve::NistP256 }),
        ("ssh_host_rsa_key", Algorithm::Rsa { hash: None }),
    ]
    .into_iter()
    .map(|(file, alg)| load_or_create_host_key(&dir.join(file), alg))
    .collect::<anyhow::Result<Vec<_>>>()?;
    let config = russh::server::Config {
        keys,
        preferred: preferred(state.config().ssh.legacy_algorithms),
        auth_rejection_time: Duration::from_secs(1),
        auth_rejection_time_initial: Some(Duration::ZERO),
        inactivity_timeout: None,
        nodelay: true,
        ..Default::default()
    };
    let listen = state.config().ssh.listen.clone();
    tracing::info!("ssh listening on {listen}");
    let mut server = SshServer { state };
    server.run_on_address(Arc::new(config), listen.as_str()).await.context("ssh server")?;
    Ok(())
}

/// russh's modern defaults, plus (if `legacy`) the older algorithms that
/// BBS terminals need: SyncTERM's cryptlib knows none of curve25519,
/// chacha20 or the ETM MACs. They go last, so clients that support
/// something better still get it.
fn preferred(legacy: bool) -> Preferred {
    let mut p = Preferred::default();
    if legacy {
        p.kex.to_mut().extend([kex::ECDH_SHA2_NISTP256, kex::DH_G14_SHA1, kex::DH_GEX_SHA1]);
        p.cipher.to_mut().extend([cipher::AES_256_CBC, cipher::AES_128_CBC]);
        p.mac.to_mut().push(mac::HMAC_SHA1);
    }
    p
}

fn load_or_create_host_key(path: &Path, alg: Algorithm) -> anyhow::Result<PrivateKey> {
    if path.exists() {
        return russh::keys::load_secret_key(path, None)
            .with_context(|| format!("loading host key {}", path.display()));
    }
    tracing::info!("generating ssh host key {}", path.display());
    let key = match alg {
        // 3072 bits: current guidance, and quicker to generate than the
        // library's 4096 default.
        Algorithm::Rsa { .. } => PrivateKey::from(RsaKeypair::random(&mut rand::rng(), 3072)?),
        alg => PrivateKey::random(&mut rand::rng(), alg)?,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    key.write_openssh_file(path, LineEnding::LF)
        .with_context(|| format!("writing host key {}", path.display()))?;
    Ok(key)
}

struct SshServer {
    state: Arc<BbsState>,
}

impl russh::server::Server for SshServer {
    type Handler = Connection;

    fn new_client(&mut self, peer: Option<SocketAddr>) -> Connection {
        Connection {
            state: self.state.clone(),
            peer,
            user: String::new(),
            channel: None,
            term: String::new(),
            size: (80, 25),
            resize: None,
        }
    }
}

/// One SSH connection; it hosts a single BBS session.
struct Connection {
    state: Arc<BbsState>,
    peer: Option<SocketAddr>,
    user: String,
    channel: Option<Channel<Msg>>,
    term: String,
    size: (u16, u16),
    resize: Option<watch::Sender<(u16, u16)>>,
}

fn dims(cols: u32, rows: u32) -> (u16, u16) {
    let clamp = |v: u32, d: u16| if v == 0 { d } else { v.min(u16::MAX as u32) as u16 };
    (clamp(cols, 80), clamp(rows, 25))
}

impl Connection {
    /// Every login is accepted (the BBS asks for its own), except from
    /// addresses that are banned or denied.
    fn auth(&self) -> Auth {
        let blocked = self.peer.and_then(|p| self.state.guard.blocked(&self.state.config().security, p.ip()));
        match blocked {
            Some(why) => {
                tracing::info!("{}: ssh refused: {why}", self.peer.map_or("?".into(), |p| p.to_string()));
                Auth::Reject { proceed_with_methods: None, partial_success: false }
            }
            None => Auth::Accept,
        }
    }
}

impl russh::server::Handler for Connection {
    type Error = anyhow::Error;

    async fn auth_none(&mut self, user: &str) -> Result<Auth, Self::Error> {
        self.user = user.to_string();
        Ok(self.auth())
    }

    async fn auth_password(&mut self, user: &str, _password: &str) -> Result<Auth, Self::Error> {
        self.user = user.to_string();
        Ok(self.auth())
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        // One BBS session per connection.
        if self.channel.is_some() || self.resize.is_some() {
            return Ok(()); // dropping `reply` rejects
        }
        self.channel = Some(channel);
        reply.accept().await;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        term: &str,
        cols: u32,
        rows: u32,
        _: u32,
        _: u32,
        _: &[(Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.term = term.to_string();
        self.size = dims(cols, rows);
        session.channel_success(channel)?;
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        _: ChannelId,
        cols: u32,
        rows: u32,
        _: u32,
        _: u32,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        self.size = dims(cols, rows);
        if let Some(tx) = &self.resize {
            let _ = tx.send(self.size);
        }
        Ok(())
    }

    async fn shell_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        let Some(ch) = self.channel.take().filter(|c| c.id() == channel) else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        session.channel_success(channel)?;
        let (tx, rx) = watch::channel(self.size);
        self.resize = Some(tx);
        let caller = Caller {
            remote: self.peer,
            transport: "ssh",
            login_name: self.user.clone(),
            term: self.term.clone(),
            size: rx,
        };
        let state = self.state.clone();
        let handle = session.handle();
        tokio::spawn(async move {
            session::run(state, Box::new(ch.into_stream()), caller).await;
            let _ = handle.eof(channel).await;
            let _ = handle.close(channel).await;
        });
        Ok(())
    }
}
