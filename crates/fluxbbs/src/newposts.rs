//! New: posts as they land, whoever wrote them (a caller, FTN, QWK, the
//! MeshCore bridge). A caller on "my boards" for that board (or a folder
//! it's in) hears of it in a line, wherever they are; and every node is
//! woken, so a list of boards or threads on screen can redraw.

use std::sync::Arc;

use tokio::sync::broadcast::error::RecvError;

use crate::BbsState;
use crate::msgareas::MsgArea;
use crate::msgbase::Message;
use crate::nodes::NodeEvent;

pub async fn watch(state: Arc<BbsState>) {
    let mut news = state.msgs.subscribe();
    loop {
        match news.recv().await {
            Ok(m) => announce(&state, &m),
            // Behind (a big toss): the lists still redraw, from the count.
            Err(RecvError::Lagged(_)) => state.nodes.wake_all(),
            Err(RecvError::Closed) => return,
        }
    }
}

fn announce(state: &BbsState, m: &Message) {
    // The notices first, so a list that redraws has them to show.
    tell(state, m);
    state.nodes.wake_all();
}

fn tell(state: &BbsState, m: &Message) {
    if m.private || m.deleted {
        return;
    }
    let areas = state.areas();
    let Some(area) = areas.areas.iter().find(|a| a.file.eq_ignore_ascii_case(&m.area)) else { return };
    let keys = lineage(&areas.areas, area);
    let from = if m.anonymous { state.strings().get("Anonymous").to_string() } else { m.from.name.clone() };
    for (node, info) in state.nodes.online() {
        if info.user.is_empty() {
            continue;
        }
        let Ok(Some(u)) = state.users.find_handle(&info.user) else { continue };
        if u.id == m.from.user_id && !m.anonymous {
            continue;
        }
        let follows = u.fav_boards.split_whitespace().any(|f| keys.iter().any(|k| k.eq_ignore_ascii_case(f)));
        if !follows || !crate::mentions::may_read(state, &u, area) {
            continue;
        }
        let text = state.strings().fmt("NewPostNotice", &[&area.name, &m.subject, &from]);
        state.nodes.send(node, NodeEvent::Notice { text });
    }
}

/// The board's key and its folders', nearest first.
fn lineage<'a>(all: &'a [MsgArea], area: &'a MsgArea) -> Vec<&'a str> {
    let mut keys = vec![area.file.as_str()];
    let mut at = area;
    while !at.parent.is_empty() && keys.len() < 16 {
        let Some(up) = all.iter().find(|a| a.file.eq_ignore_ascii_case(&at.parent)) else { break };
        keys.push(up.file.as_str());
        at = up;
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(file: &str, parent: &str) -> MsgArea {
        toml::from_str(&format!("name = '{file}'\nfile = '{file}'\nparent = '{parent}'")).expect("an area")
    }

    #[test]
    fn a_folder_counts_for_its_boards() {
        let all = vec![area("AGN", ""), area("AGNTST", "AGN"), area("AI", "")];
        assert_eq!(lineage(&all, &all[1]), ["AGNTST", "AGN"]);
        assert_eq!(lineage(&all, &all[2]), ["AI"]);
    }

    #[test]
    fn a_loop_in_the_parents_ends() {
        let all = vec![area("A", "B"), area("B", "A")];
        assert_eq!(lineage(&all, &all[0]).len(), 16);
    }
}
