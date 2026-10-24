//! New: the tarot door's pictures, in CP437 characters: one for each of
//! the major arcana, pips for the numbered cards, a figure for the court
//! cards, and the fortune teller's crystal ball. A picture is `PIC_H`
//! rows of up to `PIC_W` characters, colored by the characters in it
//! (`ink`), and a reversed card is its picture upside down.

pub const PIC_W: usize = 15;
pub const PIC_H: usize = 7;

/// A cell: a character and its color (0-15, on black).
pub type Cell = (char, u8);

struct Pic {
    rows: [&'static str; PIC_H],
    /// Characters and their colors; the first that has one wins.
    ink: &'static [(&'static str, u8)],
    /// The color of the rest.
    base: u8,
}

const MAJORS: [Pic; 22] = [
    // 0 The Fool: off the cliff's edge, sun behind, dog at heel.
    Pic {
        rows: [
            "*           * ",
            "    o  ~      ",
            "   /|\\_/      ",
            "   / \\   ▄▄,  ",
            "▓▓▓▓▓▓▓▌ ╜ ╜  ",
            "▓▓▓▓▓▓▓▌      ",
            "▀▀▀▀▀▀▀  ░  ░ ",
        ],
        ink: &[("*", 14), ("*", 15), ("~", 13), ("o/|\\_", 15), ("▄,╜", 7), ("▓▌", 6), ("░", 8)],
        base: 6,
    },
    // I The Magician: wand raised, the four suits on the table.
    Pic {
        rows: [
            "      ∞       ",
            "    \\ o       ",
            "     \\|\\      ",
            "      |       ",
            "  ▄▄▄▄▄▄▄▄▄▄▄ ",
            "  █ ♣ ♥ ♠ ♦ █ ",
            "  █         █ ",
        ],
        ink: &[("∞", 14), ("\\", 15), ("o|", 15), ("♣", 12), ("♥", 11), ("♠", 15), ("♦", 14)],
        base: 4,
    },
    // II The High Priestess: between the pillars, the moon at her feet.
    Pic {
        rows: [
            " ▐█▌   o   ▐▓▌",
            " ▐█▌  ▄█▄  ▐▓▌",
            " ▐B▌ ▐███▌ ▐J▌",
            " ▐█▌ ▐███▌ ▐▓▌",
            " ▐█▌ ▐███▌ ▐▓▌",
            "▄▐█▌▄▀▀▀▀▀▄▐▓▌",
            "≈≈≈≈≈≈≈≈≈≈≈≈≈≈",
        ],
        ink: &[("o", 15), ("B", 15), ("J", 0), ("▓", 15), ("≈", 9)],
        base: 1,
    },
    // III The Empress: crowned with stars, wheat at her feet.
    Pic {
        rows: [
            "  * * * * *   ",
            "    ▄███▄     ",
            "    █ φ █     ",
            "   ▄█████▄    ",
            "  ▐███████▌   ",
            "¥¥¥▀▀▀▀▀▀▀¥¥¥ ",
            "║║║ ≈≈≈≈≈ ║║║ ",
        ],
        ink: &[("*", 14), ("φ", 15), ("¥║", 14), ("≈", 9)],
        base: 2,
    },
    // IV The Emperor: on his stone throne.
    Pic {
        rows: [
            " ▄▄  ▲▲▲▲▲  ▄▄",
            " ██ ▐█████▌ ██",
            " ██  █ o █  ██",
            " ████▐███▌████",
            " ██ ▐█████▌ ██",
            " ██ ▐██ ██▌ ██",
            "▀▀▀▀▀▀▀▀▀▀▀▀▀▀",
        ],
        ink: &[("▲", 14), ("o", 15)],
        base: 4,
    },
    // V The Hierophant: the triple crown, the crossed keys.
    Pic {
        rows: [
            "      ▄█▄     ",
            "      ███     ",
            "  ┼  ▄███▄    ",
            "  │ ▐  o  ▌   ",
            "  │ ▐█████▌   ",
            "  │ ▐█████▌   ",
            " ═╪═  ═╪═     ",
        ],
        ink: &[("┼│", 14), ("o", 15), ("═╪", 14)],
        base: 12,
    },
    // VI The Lovers: the angel over the two of them.
    Pic {
        rows: [
            "   \\  *  /    ",
            "  ──  ♥  ──   ",
            "     ║║║      ",
            " o         o  ",
            "/|\\   ▲   /|\\ ",
            "/ \\  ▐█▌  / \\ ",
            "▄▄▄▄▄███▄▄▄▄▄▄",
        ],
        ink: &[("*\\/─", 14), ("♥", 12), ("║", 13), ("o|", 15), ("▲▐▌", 10)],
        base: 2,
    },
    // VII The Chariot: under the starry canopy, the sphinxes before it.
    Pic {
        rows: [
            "▄▄▄▄▄▄▄▄▄▄▄▄▄▄",
            "▌ * * o * * ▐ ",
            "▌    /█\\    ▐ ",
            "█▄▄▄▄███▄▄▄▄█ ",
            "▐███████████▌ ",
            " ▄▓▄     ▄█▄  ",
            " ▀ ▀     ▀ ▀  ",
        ],
        ink: &[("*", 14), ("o/\\", 15), ("▓", 8)],
        base: 3,
    },
    // VIII Strength: her hand on the lion.
    Pic {
        rows: [
            "     ∞        ",
            "     o        ",
            "    /█\\  ▄▄▄  ",
            "    ▐█▌ ▓▒▒▓▄ ",
            "    ▐█▌ ▀▓▒▓▓▌",
            "   ▄███▄ ▐▌ ▐▌",
            "▄▄▄▄▄▄▄▄▄▄▄▄▄▄",
        ],
        ink: &[("∞", 14), ("o", 15), ("/\\", 15), ("▓▒", 6), ("▐▌", 6)],
        base: 15,
    },
    // IX The Hermit: his lantern and his staff.
    Pic {
        rows: [
            "           *  ",
            "    ▄▀▄   ┌┴┐ ",
            "   ▐   ▌  │*│ ",
            "   ▐ o ▌──└─┘ ",
            "   ▐███▌      ",
            "  ▄█████▄     ",
            "▲ ███████   ▲ ",
        ],
        ink: &[("**", 14), ("┌┴┐│└─┘", 6), ("o", 15), ("▲", 15)],
        base: 8,
    },
    // X Wheel of Fortune.
    Pic {
        rows: [
            "   ▄▄▀▀▀▀▄▄   ",
            " ▄▀ \\ │ / ▀▄  ",
            " █ ── * ── █  ",
            " ▀▄ / │ \\ ▄▀  ",
            "   ▀▀▄▄▄▄▀▀   ",
            "  T A R O T   ",
            "  ▀▀▀▀▀▀▀▀▀   ",
        ],
        ink: &[("*", 14), ("\\│/─", 14), ("TARO", 15)],
        base: 6,
    },
    // XI Justice: the sword and the scales.
    Pic {
        rows: [
            "      ▲       ",
            "      │       ",
            "  ▄▄▄▄┼▄▄▄▄   ",
            "  /   │   \\   ",
            " ═══  │  ═══  ",
            "      │       ",
            "    ▄███▄     ",
        ],
        ink: &[("▲│┼", 15), ("═", 14), ("/\\", 7)],
        base: 12,
    },
    // XII The Hanged Man: by one foot, his head haloed.
    Pic {
        rows: [
            "▄▄▄▄▄▄▄▄▄▄▄▄▄▄",
            " █    │     █ ",
            " █   ┌┴┐    █ ",
            " █   │ │\\   █ ",
            " █   └┬┘    █ ",
            " █   /█\\    █ ",
            " █    *     █ ",
        ],
        ink: &[("│┌┴┐└┬┘\\", 9), ("/", 12), ("*", 14)],
        base: 6,
    },
    // XIII Death: the rider's banner and the pale horse.
    Pic {
        rows: [
            "▐▀▀▀▀▀▌       ",
            "▐  ♣  ▌  ▄▄   ",
            "▐▄▄▄▄▄▌ (°°)  ",
            "│       ▐▀▀▌  ",
            "│      ▄████▄ ",
            "│     ▀██▀▀██ ",
            "│      █    █ ",
        ],
        ink: &[("♣", 15), ("()°", 15), ("│", 7), ("▄█▀", 15)],
        base: 8,
    },
    // XIV Temperance: pouring between the cups, one foot in the water.
    Pic {
        rows: [
            "  \\   o   /   ",
            " ═ \\ ▄█▄ / ═  ",
            "    ▐███▌     ",
            "  ▄ ▐███▌ ▄   ",
            "  █≈≈≈≈≈≈≈█   ",
            "  ▀ ▐███▌ ▀   ",
            "≈≈≈≈▐█ █▌▄▄▄▄▄",
        ],
        ink: &[("o", 14), ("\\/═", 13), ("≈", 9), ("▄▀", 14)],
        base: 15,
    },
    // XV The Devil: horned on his perch, the two chained below.
    Pic {
        rows: [
            "▄▀▄   *   ▄▀▄ ",
            "  ▀▄▄▄█▄▄▄▀   ",
            " ▄▀  ▀█▀  ▀▄  ",
            "     ▐█▌      ",
            " o   ▐█▌   o  ",
            "/|\\══╧═╧══/|\\ ",
            "/ \\  ▀▀▀  / \\ ",
        ],
        ink: &[("*", 12), ("o/|\\", 7), ("═╧", 8)],
        base: 4,
    },
    // XVI The Tower: struck, its crown blown off.
    Pic {
        rows: [
            "  ⌐ ▄▲▄ ¬     ",
            " ≈  ███  \\    ",
            " *  █▒█ *     ",
            "    ███  o    ",
            " *  █▒█ /|\\   ",
            "    ███  *  ▄ ",
            "▄▄▄█████▄▄▄▄▄▄",
        ],
        ink: &[("⌐¬▲", 14), ("≈\\", 14), ("*", 12), ("▒", 12), ("o/|\\", 15)],
        base: 7,
    },
    // XVII The Star: pouring into the pool beneath the stars.
    Pic {
        rows: [
            "*    *     *  ",
            "  *     *     ",
            " *   o    *   ",
            "    /█\\       ",
            " ▄≈ ▐█▌ ≈▄    ",
            " ▀  █▀█  ▀    ",
            "≈≈≈≈≈≈≈▄▄▄▄▄▄▄",
        ],
        ink: &[("*", 14), ("*", 15), ("o", 15), ("≈", 9), ("▄▀", 14)],
        base: 11,
    },
    // XVIII The Moon: between the towers, the crayfish's path.
    Pic {
        rows: [
            "     ▄██▀     ",
            " ·  ██     ·  ",
            "     ▀██▄     ",
            "▐▓▌   ·   ▐▓▌ ",
            "▐▓▌ ■     ▐▓▌ ",
            "▓▓▓  ║  ■  ▓▓▓",
            "≈≈≈≈≈║≈≈≈≈≈≈≈≈",
        ],
        ink: &[("·", 15), ("║", 6), ("≈", 1), ("▐▓▌", 7), ("■", 6)],
        base: 14,
    },
    // XIX The Sun: the child on the horse before the wall.
    Pic {
        rows: [
            "  \\  ▄▄▄▄  /  ",
            " ── █☺☺█ ──   ",
            "  /  ▀▀▀▀  \\  ",
            " ¥     o     ¥",
            " ║    ▐▓▌    ║",
            "   ▓▓▓▓▓▓▓▓   ",
            "▒▒▒▒▒▒▒▒▒▒▒▒▒▒",
        ],
        ink: &[("\\/─", 14), ("☺", 6), ("¥", 14), ("║", 2), ("o▐▌", 15), ("▓", 15), ("▒", 12), ("▄▀█", 14)],
        base: 14,
    },
    // XX Judgement: the trumpet, and the dead rising.
    Pic {
        rows: [
            " ▄▄▄  ═══╗    ",
            "▐ o ▌════╝    ",
            " ▀█▀  ▐+▌     ",
            "  \\o/  \\o/  o ",
            "   █    █  /|\\",
            " ▄▄█▄  ▄█▄▄ ▄▄",
            "≈≈▀▀▀▀≈≈▀▀▀▀≈≈",
        ],
        ink: &[("o", 15), ("═╗╝", 14), ("+", 12), ("o\\/|", 15), ("≈", 9), ("▀", 8)],
        base: 13,
    },
    // XXI The World: dancing in the wreath.
    Pic {
        rows: [
            "    ▄▄▄▄▄     ",
            "  ▄▀  o  ▀▄   ",
            " █   /█\\   █  ",
            " █   ▐█▌   █  ",
            " █   / \\   █  ",
            "  ▀▄     ▄▀   ",
            "    ▀▀▀▀▀     ",
        ],
        ink: &[("o", 15), ("/\\", 15), ("▐▌", 13)],
        base: 10,
    },
];

/// The aces: the suit's emblem, held up large.
const ACES: [Pic; 4] = [
    // Wands: a budding staff from a cloud.
    Pic {
        rows: [
            "      ♣ ·     ",
            "     ▐█▌      ",
            "  ▄▄ ▐█▌      ",
            " ████▐█▌      ",
            "  ▀▀ ▐█▌      ",
            "     ▐█▌      ",
            "     ▀▀▀      ",
        ],
        ink: &[("♣·", 10), ("▄▀", 15)],
        base: 6,
    },
    // Cups: overflowing.
    Pic {
        rows: [
            "   ≈ ≈ ≈ ≈    ",
            "  ▄▄▄▄▄▄▄▄▄   ",
            "  █▀▀▀♥▀▀▀█   ",
            "   ▀█████▀    ",
            "     ███      ",
            "     ▐█▌      ",
            "   ▄█████▄    ",
        ],
        ink: &[("≈", 9), ("♥", 12)],
        base: 14,
    },
    // Swords: crowned.
    Pic {
        rows: [
            "    ▄▲▄       ",
            "   ( │ )      ",
            "     █        ",
            "     █        ",
            "   ══╪══      ",
            "     █        ",
            "     ▀        ",
        ],
        ink: &[("▄▲()", 14), ("═╪", 14)],
        base: 15,
    },
    // Pentacles: the coin over the garden.
    Pic {
        rows: [
            "   ▄▀▀▀▀▀▄    ",
            "  █   ▲   █   ",
            "  █ «─*─» █   ",
            "  █  / \\  █   ",
            "   ▀▄▄▄▄▄▀    ",
            "              ",
            "  ¥¥¥¥¥¥¥¥¥   ",
        ],
        ink: &[("¥", 10), ("▲«─*»/\\", 15)],
        base: 14,
    },
];

/// The court cards: page, knight, queen, king; `@` is the suit's pip.
const COURTS: [Pic; 4] = [
    Pic {
        rows: [
            "              ",
            "     ▄▄▄      ",
            "     (o)   @  ",
            "    /███\\/    ",
            "     ███      ",
            "     / \\      ",
            "▄▄▄▄▄▄▄▄▄▄▄▄▄▄",
        ],
        ink: &[("o()", 15), ("/\\", 7), ("▄", 2)],
        base: 3,
    },
    Pic {
        rows: [
            "    ▄█▄    @  ",
            "    (o)   /   ",
            "   ▄███▄ /    ",
            " ▄▄█████▄▄▄   ",
            "████████████▄ ",
            " █  ▀▀▀▀  █ ▀ ",
            " ▀        ▀   ",
        ],
        ink: &[("o()", 15), ("/", 7)],
        base: 8,
    },
    Pic {
        rows: [
            "    ▲▲▲▲      ",
            "    ▐☺▌    @  ",
            "   ▄███▄  /   ",
            "  ▐█████▌/    ",
            " ▐███████▌    ",
            " ▐███████▌    ",
            "▀▀▀▀▀▀▀▀▀▀▀▀  ",
        ],
        ink: &[("▲", 14), ("☺", 15), ("/", 7)],
        base: 5,
    },
    Pic {
        rows: [
            "  █ ▲▲▲▲▲ █   ",
            "  █  ▐☺▌  █ @ ",
            "  █ ▄███▄ █/  ",
            "  ██▐███▌██   ",
            "  █▐█████▌█   ",
            "  █ ██ ██ █   ",
            "▀▀▀▀▀▀▀▀▀▀▀▀▀ ",
        ],
        ink: &[("▲", 14), ("☺", 15), ("/", 7)],
        base: 4,
    },
];

/// The suits' pips and colors: wands, cups, swords, pentacles.
pub const PIPS: [(char, u8); 4] = [('♣', 12), ('♥', 11), ('♠', 15), ('♦', 14)];

/// The numbered cards' backdrops: wands leaves, cups ripples, swords
/// wind, pentacles coins.
const BACKDROPS: [(char, u8); 4] = [('·', 4), ('≈', 1), ('/', 8), ('·', 6)];

/// Where a numbered card's pips go: (row, column 0-2) for two to ten.
fn pip_spots(n: usize) -> Vec<(usize, usize)> {
    let pairs = |rows: &[usize]| rows.iter().flat_map(|&r| [(r, 0), (r, 2)]).collect::<Vec<_>>();
    let mut v = match n {
        2 => vec![(1, 1), (5, 1)],
        3 => vec![(0, 1), (3, 1), (6, 1)],
        4 | 5 => pairs(&[1, 5]),
        6..=8 => pairs(&[1, 3, 5]),
        _ => pairs(&[0, 2, 4, 6]),
    };
    match n {
        5 | 9 => v.push((3, 1)),
        7 => v.push((2, 1)),
        8 => v.extend([(2, 1), (4, 1)]),
        10 => v.extend([(1, 1), (5, 1)]),
        _ => {}
    }
    v
}

fn inked(p: &Pic, pip: Option<(char, u8)>) -> Vec<Vec<Cell>> {
    p.rows
        .iter()
        .map(|row| {
            let mut cells: Vec<Cell> = row
                .chars()
                .take(PIC_W)
                .map(|c| match (c, pip) {
                    ('@', Some(pp)) => pp,
                    _ => (c, p.ink.iter().find(|(cs, _)| cs.contains(c)).map_or(p.base, |&(_, k)| k)),
                })
                .collect();
            cells.resize(PIC_W, (' ', 7));
            cells
        })
        .collect()
}

/// A card's picture (the deck's order: majors, then wands, cups,
/// swords, pentacles, ace to king), upside down if reversed.
pub fn picture(card: usize, reversed: bool) -> Vec<Vec<Cell>> {
    let rows = match card {
        0..22 => inked(&MAJORS[card], None),
        _ => {
            let (suit, rank) = ((card - 22) / 14, (card - 22) % 14);
            match rank {
                0 => inked(&ACES[suit], None),
                10.. => inked(&COURTS[rank - 10], Some(PIPS[suit])),
                _ => {
                    // The pips over a faint pattern of the suit's (as the
                    // old decks had vines and flourishes round theirs).
                    let (ch, k) = BACKDROPS[suit];
                    let mut g: Vec<Vec<Cell>> = (0..PIC_H)
                        .map(|r| (0..PIC_W).map(|c| if (r + c) % 4 == 0 { (ch, k) } else { (' ', 7) }).collect())
                        .collect();
                    for (r, c) in pip_spots(rank + 1) {
                        let x = 3 + c * 4;
                        g[r][x - 1] = (' ', 7);
                        g[r][x] = PIPS[suit];
                        g[r][x + 1] = (' ', 7);
                    }
                    g
                }
            }
        }
    };
    if reversed { flip(rows) } else { rows }
}

/// Upside down: the rows in reverse, and the characters that point up or
/// down turned over.
fn flip(rows: Vec<Vec<Cell>>) -> Vec<Vec<Cell>> {
    const PAIRS: [(char, char); 8] =
        [('▀', '▄'), ('▲', '▼'), ('/', '\\'), ('┌', '└'), ('┐', '┘'), ('┬', '┴'), ('╔', '╚'), ('╗', '╝')];
    rows.into_iter()
        .rev()
        .map(|row| {
            row.into_iter()
                .map(|(c, k)| {
                    let t = PAIRS.iter().find_map(|&(a, b)| if c == a { Some(b) } else if c == b { Some(a) } else { None });
                    (t.unwrap_or(c), k)
                })
                .collect()
        })
        .collect()
}

/// The crystal ball over a fortune.
const BALL: Pic = Pic {
    rows: [
        "   ▄▄▄▄▄▄▄▄    ",
        " ▄▀░  ·  ░ ▀▄  ",
        "█░  ·   *   ░█ ",
        "█ *   ·    ░ █ ",
        " ▀▄░  ·  ░ ▄▀  ",
        "   ▐▓▓▓▓▓▓▌    ",
        " ════════════  ",
    ],
    ink: &[("▄▀█", 13), ("░", 11), ("·", 15), ("*", 14), ("▐▓▌═", 6)],
    base: 5,
};

pub fn crystal_ball() -> Vec<Vec<Cell>> {
    inked(&BALL, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CP437 glyphs below 0x20 that terminals draw rather than obey:
    /// ☺ ☻ ♥ ♦ ♣ ♠ ▲ ▼.
    const SAFE_LOW: [u8; 8] = [1, 2, 3, 4, 5, 6, 0x1E, 0x1F];

    #[test]
    fn pictures_fit_and_turn_over() {
        let all = MAJORS.iter().chain(&ACES).chain(&COURTS).chain([&BALL]);
        for row in all.flat_map(|p| p.rows) {
            assert!(row.chars().count() <= PIC_W, "{row:?} is too wide");
            // Every character is one a CP437 terminal has, and none is a
            // control byte there (CR, TAB, form feed...).
            let bytes = fluxbbs_term::cp437::encode(row);
            assert_eq!(fluxbbs_term::cp437::decode(&bytes), row);
            assert!(bytes.iter().all(|&b| b >= 0x20 && b != 0x7F || SAFE_LOW.contains(&b)), "{row:?}");
        }
        for card in 0..78 {
            let up = picture(card, false);
            assert_eq!((up.len(), up[0].len()), (PIC_H, PIC_W));
            assert_eq!(flip(picture(card, true)), up);
        }
        for n in 2..=10 {
            assert_eq!(pip_spots(n).len(), n);
        }
        assert_eq!(picture(0, true)[0][0], ('▄', 6));
    }
}
