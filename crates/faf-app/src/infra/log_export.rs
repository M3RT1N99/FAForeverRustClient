//! The client's logs as one zip a player can send: the newest rolling client
//! logs, every line scrubbed (`log_scrub`), and a short `report.txt` saying
//! which build on which device wrote them.
//!
//! Made for the Android export, where the logs sit in the app's own folder
//! and nothing but a share can get them out, and plain enough for any
//! platform.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::log_scrub::Scrubber;

/// How much raw log goes in, newest first: a few days of an ordinary client,
/// and a mail attachment once compressed.
const MAX_RAW_BYTES: u64 = 20 * 1024 * 1024;
const LOG_PREFIX: &str = "faforever-client.";
const LOG_SUFFIX: &str = ".log";

/// Write the zip of the client logs in `log_dir` to `out_dir`, replacing an
/// earlier one of the same day, and return its path.
///
/// `report` is written as `report.txt`, as it is. Lines are read leniently: a
/// log cut off mid-character by a crash still exports.
pub fn export_client_logs(
    log_dir: &Path,
    out_dir: &Path,
    file_stem: &str,
    report: &str,
    scrubber: &mut Scrubber,
) -> Result<PathBuf, String> {
    let logs = newest_logs(log_dir)?;
    std::fs::create_dir_all(out_dir)
        .map_err(|error| format!("could not make {}: {error}", out_dir.display()))?;
    let path = out_dir.join(format!("{file_stem}.zip"));
    // Written aside and moved into place, so a share never picks up half a zip.
    let partial = out_dir.join(format!("{file_stem}.zip.partial"));
    let written = write_zip(&partial, &logs, report, scrubber);
    if let Err(error) = written {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    std::fs::rename(&partial, &path)
        .map_err(|error| format!("could not finish {}: {error}", path.display()))?;
    Ok(path)
}

/// The client's own logs in `dir`, newest first, as many as fit the cap; the
/// newest always, even when it alone is larger.
fn newest_logs(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|error| format!("could not read {}: {error}", dir.display()))?;
    let mut logs: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(LOG_PREFIX) && name.ends_with(LOG_SUFFIX)
        })
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            Some((metadata.modified().ok()?, metadata.len(), entry.path()))
        })
        .collect();
    logs.sort_by(|a, b| b.0.cmp(&a.0));
    let mut total = 0;
    let mut kept = Vec::new();
    for (_, size, path) in logs {
        if !kept.is_empty() && total + size > MAX_RAW_BYTES {
            break;
        }
        total += size;
        kept.push(path);
    }
    Ok(kept)
}

fn write_zip(
    path: &Path,
    logs: &[PathBuf],
    report: &str,
    scrubber: &mut Scrubber,
) -> Result<(), String> {
    let file = File::create(path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let failed =
        |error: &dyn std::fmt::Display| format!("could not write {}: {error}", path.display());

    zip.start_file("report.txt", options)
        .map_err(|error| failed(&error))?;
    zip.write_all(report.as_bytes())
        .map_err(|error| failed(&error))?;
    for log in logs {
        let name = log
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        zip.start_file(format!("logs/{name}"), options)
            .map_err(|error| failed(&error))?;
        let mut reader = BufReader::new(File::open(log).map_err(|error| failed(&error))?);
        let mut raw = Vec::new();
        loop {
            raw.clear();
            let read = reader
                .read_until(b'\n', &mut raw)
                .map_err(|error| failed(&error))?;
            if read == 0 {
                break;
            }
            let line = String::from_utf8_lossy(&raw);
            let line = line.trim_end_matches(['\r', '\n']);
            zip.write_all(scrubber.line(line).as_bytes())
                .map_err(|error| failed(&error))?;
            zip.write_all(b"\n").map_err(|error| failed(&error))?;
        }
    }
    zip.finish().map_err(|error| failed(&error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn read_zip(path: &Path) -> Vec<(String, String)> {
        let mut archive = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        (0..archive.len())
            .map(|index| {
                let mut entry = archive.by_index(index).unwrap();
                let mut text = String::new();
                entry.read_to_string(&mut text).unwrap();
                (entry.name().unwrap().to_string(), text)
            })
            .collect()
    }

    #[test]
    fn the_client_logs_go_in_scrubbed_with_the_report_and_nothing_else() {
        let logs = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        std::fs::write(
            logs.path().join("faforever-client.2026-11-19.log"),
            "INFO lookup login=Stormlord\nINFO host 192.168.1.20\n",
        )
        .unwrap();
        std::fs::write(logs.path().join("other.txt"), "not a client log").unwrap();

        let mut scrubber = Scrubber::new(Some("Ada"), None, []);
        let path = export_client_logs(
            logs.path(),
            out.path(),
            "faf-client-logs",
            "version 1.0.0\n",
            &mut scrubber,
        )
        .unwrap();

        assert_eq!(path, out.path().join("faf-client-logs.zip"));
        assert_eq!(
            read_zip(&path),
            vec![
                ("report.txt".to_string(), "version 1.0.0\n".to_string()),
                (
                    "logs/faforever-client.2026-11-19.log".to_string(),
                    "INFO lookup login=player1\nINFO host [private]\n".to_string()
                ),
            ]
        );
        assert!(!out.path().join("faf-client-logs.zip.partial").exists());
    }

    #[test]
    fn a_log_cut_off_mid_character_still_exports() {
        let logs = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        std::fs::write(
            logs.path().join("faforever-client.2026-11-19.log"),
            b"ok\nhalf \xc3",
        )
        .unwrap();

        let mut scrubber = Scrubber::new(None, None, []);
        let path = export_client_logs(logs.path(), out.path(), "x", "", &mut scrubber).unwrap();

        let entries = read_zip(&path);
        assert_eq!(entries[1].1, "ok\nhalf \u{fffd}\n");
    }
}
