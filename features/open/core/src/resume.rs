use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use nova_files_api::uri::file_uri;

const BEFORE_FILES: [&str; 3] = ["--", "@@u", "@@"];
const DESKTOP: &str = ".desktop";

enum Start {
    Joined(&'static str),
    Split(&'static str),
}

const PLAYERS: [(&str, Start); 4] = [
    // https://github.com/celluloid-player/celluloid#command-line-options
    ("celluloid", Start::Joined("--mpv-start=")),
    // https://sourceforge.net/p/smplayer/feature-requests/728
    ("smplayer", Start::Split("-start")),
    // https://wiki.videolan.org/VLC_command-line_help/
    ("vlc", Start::Joined("--start-time=")),
    // https://mpv.io/manual/stable/#options-start
    ("mpv", Start::Joined("--start=")),
];

pub fn starting_at(
    app_id: &str,
    mut line: Vec<OsString>,
    file: &Path,
    at: Duration,
) -> Vec<OsString> {
    let id = app_id.to_ascii_lowercase();
    let id = id.strip_suffix(DESKTOP).unwrap_or(&id);
    let name = id.rsplit('.').next().unwrap_or(id);
    let Some((_, start)) = PLAYERS.iter().find(|(player, _)| *player == name) else {
        return line;
    };
    let uri = file_uri(file);
    let Some(mut place) = line
        .iter()
        .position(|word| word == file.as_os_str() || *word == *uri)
    else {
        return line;
    };
    while place > 0 && BEFORE_FILES.iter().any(|word| line[place - 1] == **word) {
        place -= 1;
    }
    let words = match start {
        Start::Joined(option) => vec![OsString::from(format!("{option}{:.3}", at.as_secs_f64()))],
        Start::Split(option) => vec![
            OsString::from(option),
            OsString::from(at.as_secs().to_string()),
        ],
    };
    line.splice(place..place, words);
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started(app_id: &str, line: &[&str]) -> Vec<String> {
        let line = line.iter().map(OsString::from).collect();
        starting_at(
            app_id,
            line,
            Path::new("/v/film.mkv"),
            Duration::from_millis(83_500),
        )
        .into_iter()
        .map(|word| word.into_string().unwrap_or_default())
        .collect()
    }

    #[test]
    fn each_player_gets_its_own_start_option_before_the_file() {
        assert_eq!(
            started(
                "mpv.desktop",
                &[
                    "mpv",
                    "--player-operation-mode=pseudo-gui",
                    "--",
                    "/v/film.mkv"
                ]
            ),
            [
                "mpv",
                "--player-operation-mode=pseudo-gui",
                "--start=83.500",
                "--",
                "/v/film.mkv"
            ]
        );
        assert_eq!(
            started(
                "vlc.desktop",
                &["/usr/bin/vlc", "--started-from-file", "file:///v/film.mkv"]
            ),
            [
                "/usr/bin/vlc",
                "--started-from-file",
                "--start-time=83.500",
                "file:///v/film.mkv"
            ]
        );
        assert_eq!(
            started(
                "io.github.celluloid_player.Celluloid.desktop",
                &["celluloid", "--new-window", "/v/film.mkv"]
            ),
            [
                "celluloid",
                "--new-window",
                "--mpv-start=83.500",
                "/v/film.mkv"
            ]
        );
        assert_eq!(
            started("smplayer.desktop", &["smplayer", "/v/film.mkv"]),
            ["smplayer", "-start", "83", "/v/film.mkv"]
        );
    }

    #[test]
    fn a_flatpak_line_or_a_name_with_spaces_still_starts_there() {
        let line = [
            "flatpak",
            "run",
            "--file-forwarding",
            "io.mpv.Mpv",
            "--",
            "@@u",
            "file:///v/My%20Film.mkv",
            "@@",
        ]
        .map(OsString::from)
        .to_vec();
        let started: Vec<String> = starting_at(
            "io.mpv.Mpv.desktop",
            line,
            Path::new("/v/My Film.mkv"),
            Duration::from_secs(2),
        )
        .into_iter()
        .map(|word| word.into_string().unwrap_or_default())
        .collect();
        assert_eq!(
            started,
            [
                "flatpak",
                "run",
                "--file-forwarding",
                "io.mpv.Mpv",
                "--start=2.000",
                "--",
                "@@u",
                "file:///v/My%20Film.mkv",
                "@@",
            ]
        );
    }

    #[test]
    fn another_app_or_a_line_without_the_file_is_left_as_it_is() {
        assert_eq!(
            started("org.kde.haruna.desktop", &["haruna", "/v/film.mkv"]),
            ["haruna", "/v/film.mkv"]
        );
        assert_eq!(
            started("io.github.GnomeMpv.desktop", &["gnome-mpv", "/v/film.mkv"]),
            ["gnome-mpv", "/v/film.mkv"]
        );
        assert_eq!(started("mpv.desktop", &["mpv"]), ["mpv"]);
    }
}
