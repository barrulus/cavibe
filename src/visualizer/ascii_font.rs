//! ASCII art font rendering for text display
//!
//! Provides two font styles:
//! - `Ascii`: Simple 3-line block letters using Unicode box characters
//! - `Figlet`: Larger 5-line banner text similar to figlet output

/// Height of ASCII style font in terminal rows
pub const ASCII_HEIGHT: u16 = 3;

/// Height of Figlet style font in terminal rows
pub const FIGLET_HEIGHT: u16 = 5;

/// ASCII style: 3-line block letters using simple characters
pub fn get_ascii_char(c: char) -> Option<[&'static str; 3]> {
    let c = c.to_ascii_uppercase();
    Some(match c {
        'A' => [" A ", "A A", "AAA"],
        'B' => ["BB ", "BB ", "BB "],
        'C' => [" CC", "C  ", " CC"],
        'D' => ["DD ", "D D", "DD "],
        'E' => ["EEE", "EE ", "EEE"],
        'F' => ["FFF", "FF ", "F  "],
        'G' => [" GG", "G G", "GGG"],
        'H' => ["H H", "HHH", "H H"],
        'I' => ["III", " I ", "III"],
        'J' => ["JJJ", "  J", "JJ "],
        'K' => ["K K", "KK ", "K K"],
        'L' => ["L  ", "L  ", "LLL"],
        'M' => ["M M", "MMM", "M M"],
        'N' => ["N N", "NNN", "N N"],
        'O' => [" O ", "O O", " O "],
        'P' => ["PP ", "PP ", "P  "],
        'Q' => [" Q ", "Q Q", " QQ"],
        'R' => ["RR ", "RR ", "R R"],
        'S' => [" SS", " S ", "SS "],
        'T' => ["TTT", " T ", " T "],
        'U' => ["U U", "U U", "UUU"],
        'V' => ["V V", "V V", " V "],
        'W' => ["W W", "WWW", "W W"],
        'X' => ["X X", " X ", "X X"],
        'Y' => ["Y Y", " Y ", " Y "],
        'Z' => ["ZZZ", " Z ", "ZZZ"],
        '0' => [" 0 ", "0 0", " 0 "],
        '1' => [" 1 ", " 1 ", " 1 "],
        '2' => ["22 ", " 2 ", "222"],
        '3' => ["33 ", " 33", "33 "],
        '4' => ["4 4", "444", "  4"],
        '5' => ["555", "55 ", " 55"],
        '6' => [" 66", "66 ", "666"],
        '7' => ["777", "  7", "  7"],
        '8' => [" 8 ", "8 8", " 8 "],
        '9' => ["999", " 99", " 99"],
        ' ' => ["   ", "   ", "   "],
        '-' => ["   ", "---", "   "],
        '.' => ["   ", "   ", " . "],
        '!' => [" ! ", " ! ", " ! "],
        '?' => ["?? ", " ? ", " ? "],
        '\'' => [" ' ", "   ", "   "],
        '"' => ["\" \"", "   ", "   "],
        '(' => [" ( ", "(  ", " ( "],
        ')' => [" ) ", "  )", " ) "],
        '&' => [" & ", "& &", " &&"],
        _ => return None,
    })
}

/// Figlet style: 5-line banner text (simplified "small" style)
pub fn get_figlet_char(c: char) -> Option<[&'static str; 5]> {
    let c = c.to_ascii_uppercase();
    Some(match c {
        'A' => [
            "  _   ",
            " / \\  ",
            "/ _ \\ ",
            "| |_| |",
            "|_| |_|",
        ],
        'B' => [
            " ___  ",
            "| _ ) ",
            "| _ \\ ",
            "|___/ ",
            "      ",
        ],
        'C' => [
            "  ___ ",
            " / __|",
            "| (__ ",
            " \\___|",
            "      ",
        ],
        'D' => [
            " ___  ",
            "|   \\ ",
            "| |) |",
            "|___/ ",
            "      ",
        ],
        'E' => [
            " ___ ",
            "| __|",
            "| _| ",
            "|___|",
            "     ",
        ],
        'F' => [
            " ___ ",
            "| __|",
            "| _| ",
            "|_|  ",
            "     ",
        ],
        'G' => [
            "  ___ ",
            " / __|",
            "| (_ |",
            " \\___|",
            "      ",
        ],
        'H' => [
            " _  _ ",
            "| || |",
            "| __ |",
            "|_||_|",
            "      ",
        ],
        'I' => [
            " ___ ",
            "|_ _|",
            " | | ",
            "|___|",
            "     ",
        ],
        'J' => [
            "    _ ",
            " _ | |",
            "| || |",
            " \\__/ ",
            "      ",
        ],
        'K' => [
            " _  __",
            "| |/ /",
            "| ' < ",
            "|_|\\_\\",
            "      ",
        ],
        'L' => [
            " _    ",
            "| |   ",
            "| |__ ",
            "|____|",
            "      ",
        ],
        'M' => [
            " __  __ ",
            "|  \\/  |",
            "| |\\/| |",
            "|_|  |_|",
            "        ",
        ],
        'N' => [
            " _  _ ",
            "| \\| |",
            "| .` |",
            "|_|\\_|",
            "      ",
        ],
        'O' => [
            "  ___  ",
            " / _ \\ ",
            "| (_) |",
            " \\___/ ",
            "       ",
        ],
        'P' => [
            " ___ ",
            "| _ \\",
            "|  _/",
            "|_|  ",
            "     ",
        ],
        'Q' => [
            "  ___  ",
            " / _ \\ ",
            "| (_) |",
            " \\__\\_\\",
            "       ",
        ],
        'R' => [
            " ___ ",
            "| _ \\",
            "|   /",
            "|_|_\\",
            "     ",
        ],
        'S' => [
            " ___ ",
            "/ __|",
            "\\__ \\",
            "|___/",
            "     ",
        ],
        'T' => [
            " _____ ",
            "|_   _|",
            "  | |  ",
            "  |_|  ",
            "       ",
        ],
        'U' => [
            " _   _ ",
            "| | | |",
            "| |_| |",
            " \\___/ ",
            "       ",
        ],
        'V' => [
            "__   __",
            "\\ \\ / /",
            " \\ V / ",
            "  \\_/  ",
            "       ",
        ],
        'W' => [
            "__      __",
            "\\ \\    / /",
            " \\ \\/\\/ / ",
            "  \\_/\\_/  ",
            "          ",
        ],
        'X' => [
            "__  __",
            "\\ \\/ /",
            " >  < ",
            "/_/\\_\\",
            "      ",
        ],
        'Y' => [
            "__   __",
            "\\ \\ / /",
            " \\ V / ",
            "  |_|  ",
            "       ",
        ],
        'Z' => [
            " ____",
            "|_  /",
            " / / ",
            "/___|",
            "     ",
        ],
        '0' => [
            "  __  ",
            " /  \\ ",
            "| () |",
            " \\__/ ",
            "      ",
        ],
        '1' => [
            " _ ",
            "/ |",
            "| |",
            "|_|",
            "   ",
        ],
        '2' => [
            " ___ ",
            "|_  )",
            " / / ",
            "/___|",
            "     ",
        ],
        '3' => [
            " ____",
            "|__ /",
            " |_ \\",
            "|___/",
            "     ",
        ],
        '4' => [
            " _ _  ",
            "| | | ",
            "|_  _|",
            "  |_| ",
            "      ",
        ],
        '5' => [
            " ___ ",
            "| __|",
            "|__ \\",
            "|___/",
            "     ",
        ],
        '6' => [
            "  __ ",
            " / / ",
            "/ _ \\",
            "\\___/",
            "     ",
        ],
        '7' => [
            " ____ ",
            "|__  |",
            "  / / ",
            " /_/  ",
            "      ",
        ],
        '8' => [
            " ___ ",
            "( _ )",
            "/ _ \\",
            "\\___/",
            "     ",
        ],
        '9' => [
            " ___ ",
            "/ _ \\",
            "\\_, /",
            " /_/ ",
            "     ",
        ],
        ' ' => [
            "    ",
            "    ",
            "    ",
            "    ",
            "    ",
        ],
        '-' => [
            "     ",
            "     ",
            " ___ ",
            "|___|",
            "     ",
        ],
        '.' => [
            "   ",
            "   ",
            " _ ",
            "(_)",
            "   ",
        ],
        '!' => [
            " _ ",
            "| |",
            "|_|",
            "(_)",
            "   ",
        ],
        '?' => [
            " ___ ",
            "|__ \\",
            "  /_/",
            " (_) ",
            "     ",
        ],
        '\'' => [
            " _ ",
            "( )",
            "|/ ",
            "   ",
            "   ",
        ],
        _ => return None,
    })
}

/// Render a string in ASCII style (3 rows high)
/// Returns a vector of strings, one per row
pub fn render_ascii(text: &str) -> Vec<String> {
    let mut rows = vec![String::new(); ASCII_HEIGHT as usize];

    for c in text.chars() {
        if let Some(glyph) = get_ascii_char(c) {
            for (i, row) in glyph.iter().enumerate() {
                rows[i].push_str(row);
            }
        } else {
            // Unknown char: use space
            for row in &mut rows {
                row.push_str("   ");
            }
        }
    }

    rows
}

/// Render a string in Figlet style (5 rows high)
/// Returns a vector of strings, one per row
pub fn render_figlet(text: &str) -> Vec<String> {
    let mut rows = vec![String::new(); FIGLET_HEIGHT as usize];

    for c in text.chars() {
        if let Some(glyph) = get_figlet_char(c) {
            // Find max width of this character's glyph
            let max_width = glyph.iter().map(|r| r.chars().count()).max().unwrap_or(0);

            for (i, row) in glyph.iter().enumerate() {
                let row_len = row.chars().count();
                rows[i].push_str(row);
                // Pad to max width to ensure all rows are same length
                for _ in row_len..max_width {
                    rows[i].push(' ');
                }
            }
        } else {
            // Unknown char: use space
            for row in &mut rows {
                row.push_str("    ");
            }
        }
    }

    rows
}

/// Get the width of a rendered ASCII string
pub fn ascii_width(text: &str) -> usize {
    text.chars()
        .map(|c| get_ascii_char(c).map(|g| g[0].len()).unwrap_or(3))
        .sum()
}

/// Get the width of a rendered Figlet string
pub fn figlet_width(text: &str) -> usize {
    text.chars()
        .map(|c| {
            get_figlet_char(c)
                .map(|g| g.iter().map(|r| r.chars().count()).max().unwrap_or(0))
                .unwrap_or(4)
        })
        .sum()
}
