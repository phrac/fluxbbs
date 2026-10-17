//! Multinode interaction (NODES.PAS): messages to one node (`XM`) or all
//! of them (`XB`), multinode chat (`XC`), and setting what the node list
//! says you're doing (`XS`).

use std::io;

use iniquity_term::Slot;
use iniquity_term::codes::squish;

use crate::input::{Key, ReadOpts};
use crate::nodes::NodeEvent;
use crate::session::{Session, waves};

/// Longest chat line or node message (a Pascal string).
const MAX_TEXT: usize = 255;

/// `ncOutWrap`: split chat text into lines of at most `width` columns,
/// at spaces where possible.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word = word.to_string();
        while word.chars().count() > width {
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            let head: String = word.chars().take(width).collect();
            word = word.chars().skip(width).collect();
            out.push(head);
        }
        let need = if line.is_empty() { word.chars().count() } else { line.chars().count() + 1 + word.chars().count() };
        if need > width && !line.is_empty() {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// Where the chat screen puts things.
struct ChatLayout {
    /// First and last rows of the conversation.
    top: u16,
    bottom: u16,
    /// The input line: start column, row, width.
    x: u16,
    y: u16,
    width: usize,
}

impl Session {
    /// Show node messages that have arrived (`nodeRead` for `nd_msg`).
    pub fn show_node_messages(&mut self) {
        for ev in self.node.take(false) {
            if let NodeEvent::Message { from, user, text, broadcast } = ev {
                let name = if broadcast { "NodeBCreceived" } else { "NodeMsgReceived" };
                let head = self.strings().fmt(name, &[&user, &from.to_string()]);
                if self.screen.where_x() != 1 {
                    self.newline();
                }
                self.newline();
                self.write_with_controls(&head);
                self.write_with_controls(&text);
                self.set_slot(Slot::Text);
                self.newline();
            }
        }
    }

    fn only_node(&self) -> bool {
        self.state.config().bbs.max_nodes <= 1
    }

    /// `nodeMsg` (`XM`): send a line to one node, by number or handle.
    pub async fn node_message(&mut self) -> io::Result<()> {
        if self.only_node() {
            return self.say_line("NodeNotMulti").await;
        }
        let want = loop {
            self.say("NodeAskNode").await?;
            let s = self.read_string(ReadOpts::new(36)).await?;
            self.newline();
            if s != "?" {
                break s;
            }
            self.node_list().await?;
        };
        if want.is_empty() {
            return Ok(());
        }
        let nodes = self.node.nodes().clone();
        let to = want.parse::<usize>().ok().or_else(|| nodes.find_user(&want));
        let online = nodes.online();
        let Some((n, info)) = to.and_then(|n| online.into_iter().find(|(m, _)| *m == n)) else { return Ok(()) };
        if n == self.node.number() || info.user.is_empty() {
            return Ok(());
        }
        let s = self.strings().fmt("NodeUserOn", &[&n.to_string(), &info.user, info.transport]);
        self.show_line(&s).await?;
        self.say("NodeMsgAskText").await?;
        let text = self.read_string(ReadOpts::new(MAX_TEXT)).await?;
        self.newline();
        if text.is_empty() {
            return Ok(());
        }
        let ev = NodeEvent::Message { from: self.node.number(), user: self.user.handle.clone(), text, broadcast: false };
        if nodes.send(n, ev) {
            let s = self.strings().fmt("NodeMsgSent", &[&info.user, &n.to_string()]);
            self.show_line(&s).await?;
        }
        Ok(())
    }

    /// `nodeBroadcast` (`XB`): send a line to every other node.
    pub async fn node_broadcast(&mut self) -> io::Result<()> {
        if self.only_node() {
            return self.say_line("NodeNotMulti").await;
        }
        self.say("NodeBCaskText").await?;
        let text = self.read_string(ReadOpts::new(MAX_TEXT)).await?;
        self.newline();
        if text.is_empty() {
            return Ok(());
        }
        let ev = NodeEvent::Message { from: self.node.number(), user: self.user.handle.clone(), text, broadcast: true };
        self.node.nodes().send_all(self.node.number(), ev);
        tracing::info!("{}: broadcast to all nodes", self.user.handle);
        self.say_line("NodeBCsent").await
    }

    /// `nodeUpdate` (`XS`): what the node list says this node is doing.
    pub fn node_status(&mut self, text: &str) {
        let text = text.trim().to_string();
        if !text.is_empty() {
            self.node.update(|n| n.activity = text);
        }
    }

    /// `nodeChat` (`XC`): talk with everyone else in chat. `/quit` (or
    /// `/exit`, `/bye`, or Esc) leaves.
    pub async fn node_chat(&mut self) -> io::Result<()> {
        if self.only_node() {
            return self.say_line("NodeNotMulti").await;
        }
        let me = (self.node.number(), self.user.handle.clone());
        self.node.update(|n| {
            n.in_chat = true;
            n.activity = "Multinode chat".into();
        });
        self.hold_node_msgs = true;
        let layout = self.chat_screen().await?;
        let nodes = self.node.nodes().clone();
        nodes.send_all(me.0, NodeEvent::ChatJoin { user: me.1.clone() });
        let mut lines: Vec<String> = Vec::new();
        let joined = self.strings().fmt("MNCenterchat", &[&me.1]);
        self.chat_out(&layout, &mut lines, &joined);
        let waker = self.node.waker();
        let mut input = String::new();
        self.chat_input(&layout, &input);
        loop {
            self.flush().await?;
            tokio::select! {
                key = self.read_key() => match key? {
                    Key::Esc => break,
                    Key::Enter => {
                        let text = std::mem::take(&mut input);
                        let cmd = text.strip_prefix('/').map(|c| c.trim().to_ascii_lowercase());
                        if matches!(cmd.as_deref(), Some("quit" | "exit" | "bye" | "q")) {
                            break;
                        }
                        if !text.trim().is_empty() && cmd.is_none() {
                            let said = self.strings().fmt("MNCyouspeak", &[&me.1, &text]);
                            self.chat_out(&layout, &mut lines, &said);
                            nodes.send_all(me.0, NodeEvent::Chat { from: me.0, user: me.1.clone(), text });
                        }
                    }
                    Key::Backspace => {
                        input.pop();
                    }
                    Key::Char(0x15 | 0x18 | 0x19) => input.clear(),
                    Key::Char(c @ 0x20..=0x7E) if input.chars().count() < MAX_TEXT => input.push(c as char),
                    Key::Char(c @ 0x80..=0xFF) if input.chars().count() < MAX_TEXT => input.push(iniquity_term::cp437::TO_UNICODE[c as usize]),
                    _ => {}
                },
                _ = waker.notified() => {}
            }
            let mut kicked = false;
            for ev in self.node.take(true) {
                let line = match ev {
                    NodeEvent::Kick { text } => {
                        kicked = true;
                        text
                    }
                    NodeEvent::Chat { user, text, .. } => self.strings().fmt("MNCtheyspeak", &[&user, &text]),
                    NodeEvent::ChatJoin { user } => self.strings().fmt("MNCenterchat", &[&user]),
                    NodeEvent::ChatPart { user } => self.strings().fmt("MNCquitchat", &[&user]),
                    NodeEvent::Message { from, user, text, broadcast } => {
                        let name = if broadcast { "NodeBCreceived" } else { "NodeMsgReceived" };
                        format!("{}{text}", self.strings().fmt(name, &[&user, &from.to_string()]))
                    }
                };
                self.chat_out(&layout, &mut lines, &line);
            }
            if kicked {
                // Leave chat properly, then go.
                self.hangup = true;
                break;
            }
            self.chat_input(&layout, &input);
        }
        nodes.send_all(me.0, NodeEvent::ChatPart { user: me.1.clone() });
        self.node.update(|n| {
            n.in_chat = false;
            n.activity = format!("Menu: {}", self.menu.current);
        });
        self.hold_node_msgs = false;
        self.set_slot(Slot::Text);
        self.screen.goto(1, self.screen.rows());
        self.newline();
        if self.hangup {
            self.flush().await?;
            return Err(crate::session::kicked());
        }
        Ok(())
    }

    /// Draw NODECHAT (its `~TL`/`~BL` mark the conversation, `~SI`/`~EI`
    /// the input line), or a plain screen without it.
    async fn chat_screen(&mut self) -> io::Result<ChatLayout> {
        self.set_slot(Slot::Text);
        self.clear_screen();
        let w = waves([("ND", self.node.number().to_string()), ("UN", self.user.handle.clone())]);
        if self.show_file_with("NODECHAT", &w).await?
            && let (Some(tl), Some(bl), Some(si), Some(ei)) =
                (self.positions.get(b"TL"), self.positions.get(b"BL"), self.positions.get(b"SI"), self.positions.get(b"EI"))
            && bl.1 >= tl.1
            && ei.0 >= si.0
        {
            return Ok(ChatLayout { top: tl.1, bottom: bl.1, x: si.0, y: si.1, width: (ei.0 - si.0 + 1) as usize });
        }
        let rows = self.screen.rows();
        self.clear_screen();
        self.screen.goto(1, rows - 1);
        self.set_slot(Slot::Border);
        self.write(&"─".repeat(79));
        Ok(ChatLayout { top: 1, bottom: rows - 2, x: 1, y: rows, width: 79 })
    }

    /// Add to the conversation; when the window fills, the older half
    /// scrolls away (`ncOut`).
    fn chat_out(&mut self, l: &ChatLayout, lines: &mut Vec<String>, text: &str) {
        let height = (l.bottom - l.top + 1) as usize;
        for line in wrap(text, 79) {
            if lines.len() >= height {
                let keep = lines.split_off(height / 2 + 1);
                *lines = keep;
                for i in 0..height {
                    self.screen.goto(1, l.top + i as u16);
                    self.screen.clear_eol();
                    if let Some(s) = lines.get(i) {
                        self.set_slot(Slot::Info);
                        self.write_with_controls(s);
                    }
                }
            }
            self.screen.goto(1, l.top + lines.len() as u16);
            self.set_slot(Slot::Info);
            self.write_with_controls(&line);
            lines.push(line);
        }
    }

    /// Redraw the input line (its tail, if it's longer than the space).
    fn chat_input(&mut self, l: &ChatLayout, input: &str) {
        let skip = input.chars().count().saturating_sub(l.width.saturating_sub(1));
        let shown: String = input.chars().skip(skip).collect();
        self.screen.goto(l.x, l.y);
        self.set_slot(Slot::TextHi);
        self.write(&format!("{:<width$}", squish(&shown, l.width), width = l.width));
        self.screen.goto(l.x + shown.chars().count() as u16, l.y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping() {
        assert_eq!(wrap("hello there you", 11), ["hello there", "you"]);
        assert_eq!(wrap("  spaced   out  ", 20), ["spaced out"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert!(wrap("   ", 10).is_empty());
    }
}
