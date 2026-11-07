//! SSH transport. Authentication is left to the BBS's own logon screens,
//! as with a telnet or modem caller, so every SSH auth attempt is accepted
//! and the SSH user name is only used as a suggested handle. New: when
//! the SSH user name is an account (a handle, or the SSH user name it
//! chose), the client is asked for a key or a password first: a key on
//! file (`sshkeys`), or the account's own password (a terminal's stored
//! login, as SyncTERM keeps), logs straight in; anything else falls back
//! to the board's own login.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use russh::keys::ssh_key::LineEnding;
use russh::keys::ssh_key::private::RsaKeypair;
use russh::keys::{Algorithm, EcdsaCurve, PrivateKey};
use russh::{Preferred, cipher, kex, mac};
use russh::server::{Auth, ChannelOpenHandle, Msg, Response, Server as _, Session};
use russh::{MethodKind, MethodSet};
use russh::{Channel, ChannelId, Pty};
use tokio::sync::{oneshot, watch};

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
        // A connection that sends nothing for ten minutes (one stuck
        // before the login, a dead link) goes; a keepalive a minute is a
        // nudge, never a reason to drop (SyncTERM's cryptlib may not
        // answer them). Idle callers go by the BBS's own timeout.
        inactivity_timeout: Some(Duration::from_secs(600)),
        keepalive_interval: Some(Duration::from_secs(60)),
        keepalive_max: 0,
        nodelay: true,
        ..Default::default()
    };
    let listen = state.config().ssh.listen.clone();
    tracing::info!("ssh listening on {listen}");
    let listener = tokio::net::TcpListener::bind(&listen).await.with_context(|| format!("ssh on {listen}"))?;
    let config = Arc::new(config);
    let mut server = SshServer { state: state.clone() };
    // russh's `run_on_socket`, but banned and denied addresses are
    // dropped as telnet drops them: before the handshake, without a word.
    loop {
        // New: an accept error (out of descriptors) waits a moment and
        // goes on, rather than ending the server.
        let (sock, peer) = match listener.accept().await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("ssh: accept: {e}");
                tokio::time::sleep(Duration::from_millis(250)).await;
                continue;
            }
        };
        if let Some(why) = crate::logon::turned_away(&state, peer) {
            tracing::info!("{peer}: ssh refused: {why}");
            drop(sock);
            continue;
        }
        // New: connections that haven't asked for a shell yet are counted
        // per address, and have `PRE_SESSION` to get there.
        let Some(pending) = state.guard.pending(&state.config().security, peer.ip()) else {
            tracing::info!("{peer}: ssh refused: too many connections logging in");
            drop(sock);
            continue;
        };
        let _ = sock.set_nodelay(true);
        let (started_tx, started_rx) = oneshot::channel();
        let mut handler = server.new_client(Some(peer));
        handler.pending = Some(pending);
        handler.started = Some(started_tx);
        let config = config.clone();
        tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + PRE_SESSION;
            let mut session = match tokio::time::timeout_at(deadline, russh::server::run_stream(config, sock, handler)).await {
                Ok(Ok(session)) => session,
                Ok(Err(e)) => return tracing::debug!("{peer}: ssh setup: {e:#}"),
                Err(_) => return tracing::debug!("{peer}: ssh setup took too long"),
            };
            let handle = session.handle();
            let started = tokio::select! {
                r = &mut session => {
                    if let Err(e) = r {
                        tracing::debug!("{peer}: ssh: {e:#}");
                    }
                    return;
                }
                s = tokio::time::timeout_at(deadline, started_rx) => matches!(s, Ok(Ok(()))),
            };
            if !started {
                tracing::info!("{peer}: ssh: no shell within {}s, closed", PRE_SESSION.as_secs());
                let _ = handle.disconnect(russh::Disconnect::ByApplication, "took too long to log in".into(), String::new()).await;
            }
            if let Err(e) = session.await {
                tracing::debug!("{peer}: ssh: {e:#}");
            }
        });
    }
}

/// New: how long a connection has from connecting to asking for a shell
/// (the handshake, and a person typing a password at their client's
/// prompt).
const PRE_SESSION: Duration = Duration::from_secs(120);

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
            key_user: None,
            ssh_password: None,
            pending: None,
            started: None,
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
    /// The user whose key or password logged this connection in.
    key_user: Option<u32>,
    /// The password, if it was one (`Caller::ssh_password`).
    ssh_password: Option<String>,
    /// Counted as not yet a session (`Guard::pending`) until the shell.
    pending: Option<crate::guard::Pending>,
    /// Told when the shell starts, so `serve` stops the clock.
    started: Option<oneshot::Sender<()>>,
    channel: Option<Channel<Msg>>,
    term: String,
    size: (u16, u16),
    resize: Option<watch::Sender<(u16, u16)>>,
}

fn dims(cols: u32, rows: u32) -> (u16, u16) {
    let clamp = |v: u32, d: u16| if v == 0 { d } else { v.min(u16::MAX as u32) as u16 };
    crate::telnet::clamp_size(clamp(cols, 80), clamp(rows, 25))
}

impl Connection {
    /// Every login is accepted (the BBS asks for its own), except from
    /// addresses that are banned or denied.
    /// (Banned when it connected, it was dropped then; this is for a ban
    /// that began since.)
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

    /// The account `user` names: its handle, or the SSH user name it chose.
    fn account(&self, user: &str) -> Option<crate::user::User> {
        let users = &self.state.users;
        users.find(user, false).ok().flatten().or_else(|| users.by_ssh_user(user).ok().flatten()).filter(|u| !u.deleted)
    }

    /// [`Self::account`], if it has SSH keys on file.
    fn keyed_user(&self, user: &str) -> Option<crate::user::User> {
        self.account(user).filter(|u| !u.ssh_keys.is_empty())
    }
}

impl russh::server::Handler for Connection {
    type Error = anyhow::Error;

    async fn auth_none(&mut self, user: &str) -> Result<Auth, Self::Error> {
        self.user = user.to_string();
        let auth = self.auth();
        // A name that's an account: ask for a key or a password first, which
        // may log straight in. Without either, keyboard-interactive (no
        // questions) still gets in, to the board's own login.
        if matches!(auth, Auth::Accept) && self.account(user).is_some() {
            let methods: &[MethodKind] = &[MethodKind::PublicKey, MethodKind::KeyboardInteractive, MethodKind::Password];
            return Ok(Auth::Reject { proceed_with_methods: Some(MethodSet::from(methods)), partial_success: false });
        }
        Ok(auth)
    }

    async fn auth_publickey_offered(&mut self, user: &str, key: &russh::keys::PublicKey) -> Result<Auth, Self::Error> {
        let ok = self.keyed_user(user).is_some_and(|u| crate::sshkeys::matches(&u.ssh_keys, key));
        Ok(if ok { self.auth() } else { Auth::reject() })
    }

    async fn auth_publickey(&mut self, user: &str, key: &russh::keys::PublicKey) -> Result<Auth, Self::Error> {
        self.user = user.to_string();
        match self.keyed_user(user).filter(|u| crate::sshkeys::matches(&u.ssh_keys, key)) {
            Some(u) if matches!(self.auth(), Auth::Accept) => {
                tracing::info!("{}: ssh key accepted for {}", self.peer.map_or("?".into(), |p| p.to_string()), u.handle);
                self.key_user = Some(u.id);
                self.ssh_password = None;
                Ok(Auth::Accept)
            }
            _ => Ok(Auth::reject()),
        }
    }

    /// No questions: the board asks its own.
    async fn auth_keyboard_interactive<'a>(&'a mut self, user: &str, _submethods: &str, _response: Option<Response<'a>>) -> Result<Auth, Self::Error> {
        self.user = user.to_string();
        Ok(self.auth())
    }

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        self.user = user.to_string();
        let auth = self.auth();
        // New: the account's own password logs straight in; a wrong one
        // counts as a failed login. Either way (and for names that aren't
        // accounts) the board's own login follows if it didn't.
        if matches!(auth, Auth::Accept) && !password.is_empty()
            && let Some(u) = self.account(user)
        {
            if crate::logon::password_ok(&u, password).await {
                tracing::info!("{}: ssh password accepted for {}", self.peer.map_or("?".into(), |p| p.to_string()), u.handle);
                self.key_user = Some(u.id);
                // The session forgets the address's failures once it's in.
                self.ssh_password = Some(password.to_string());
            } else if let Some(p) = self.peer
                && crate::logon::record_failure_for(&self.state, p, &format!("wrong ssh password for {}", u.handle), Some(u.id))
            {
                return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false });
            }
        }
        Ok(auth)
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
        if !term.is_empty() {
            self.term = term.to_string();
        }
        self.size = dims(cols, rows);
        session.channel_success(channel)?;
        Ok(())
    }

    // Every channel request a client wants a reply to gets one (success or
    // failure); clients on libssh2 and DeuceSSH (SyncTERM 2) ask for one on
    // `env` and wait for it. Russh only sends the reply when one was asked for.
    async fn env_request(&mut self, channel: ChannelId, name: &str, value: &str, session: &mut Session) -> Result<(), Self::Error> {
        if name == "TERM" && !value.is_empty() {
            if self.term.is_empty() {
                self.term = value.to_string();
            }
            session.channel_success(channel)?;
        } else {
            session.channel_failure(channel)?;
        }
        Ok(())
    }

    async fn x11_request(&mut self, channel: ChannelId, _: bool, _: &str, _: &str, _: u32, session: &mut Session) -> Result<(), Self::Error> {
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn exec_request(&mut self, channel: ChannelId, _: &[u8], session: &mut Session) -> Result<(), Self::Error> {
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn subsystem_request(&mut self, channel: ChannelId, _: &str, session: &mut Session) -> Result<(), Self::Error> {
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        cols: u32,
        rows: u32,
        _: u32,
        _: u32,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.size = dims(cols, rows);
        if let Some(tx) = &self.resize {
            let _ = tx.send(self.size);
        }
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        let Some(ch) = self.channel.take().filter(|c| c.id() == channel) else {
            session.channel_failure(channel)?;
            return Ok(());
        };
        session.channel_success(channel)?;
        self.pending = None;
        if let Some(tx) = self.started.take() {
            let _ = tx.send(());
        }
        let (tx, rx) = watch::channel(self.size);
        self.resize = Some(tx);
        let caller = Caller {
            remote: self.peer,
            transport: "ssh",
            login_name: self.user.clone(),
            key_user: self.key_user,
            ssh_password: self.ssh_password.take(),
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
