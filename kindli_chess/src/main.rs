use std::process::Command;
use std::thread;
use std::time::Duration;

// use log::LevelFilter;
// use log::info;
// use log4rs::append::file::FileAppender;
// use log4rs::config::{Appender, Config, Root};
// use log4rs::encode::pattern::PatternEncoder;

fn main() {
    // LOGGING
    // 1. Appender für die Datei definieren
    // let logfile = FileAppender::builder()
    //     .encoder(Box::new(PatternEncoder::new("{d} - {m}{n}")))
    //     .build("output.log")
    //     .unwrap();

    // // 2. Logging-Konfiguration erstellen
    // let config = Config::builder()
    //     .appender(Appender::builder().build("logfile", Box::new(logfile)))
    //     .build(Root::builder().appender("logfile").build(LevelFilter::Info))
    //     .unwrap();

    // // 3. Logger initialisieren
    // log4rs::init_config(config).unwrap();

    // // 4. Loslegen mit dem Logging
    // println!("Anwendung gestartet. Log-Nachrichten werden in 'output.log' geschrieben.");

    // APPLICATION
    println!("Kindle Hello World App gestartet");

    // SICHERHEIT: Falls etwas schiefgeht, haben wir nur 10 Sekunden "Schaden"
    let result = run_hello_world();

    match result {
        Ok(_) => println!("App erfolgreich beendet"),
        Err(e) => {
            println!("Fehler aufgetreten: {}", e);
            // Bei Fehler: Sofort beenden, nichts kaputt machen
        }
    }
}

fn run_hello_world() -> Result<(), Box<dyn std::error::Error>> {
    // Schritt 1: "Hello World" in der rechten oberen Ecke anzeigen
    show_text_top_right("Hello World")?;

    println!("Text angezeigt - warte 10 Sekunden...");

    // Schritt 2: 10 Sekunden warten
    thread::sleep(Duration::from_secs(10));

    // Schritt 3: Display löschen (zurück zum normalen Zustand)
    clear_display()?;

    println!("Display gelöscht - App beendet sich");

    Ok(())
}

fn show_text_top_right(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Verwende das 'eips' Tool (falls verfügbar) - sehr sicher
    let output = Command::new("eips")
        .args(&[
            "-x", "800", // X-Position (rechts)
            "-y", "50", // Y-Position (oben)
            "-h", "100", // Höhe
            text,
        ])
        .output();

    match output {
        Ok(result) => {
            if result.status.success() {
                println!("Text erfolgreich mit eips angezeigt");
                return Ok(());
            }
        }
        Err(_) => {
            println!("eips nicht verfügbar - versuche Alternative");
        }
    }

    // Fallback: Einfacher Framebuffer-Zugriff (sehr vorsichtig)
    simple_framebuffer_text(text)?;

    Ok(())
}

fn simple_framebuffer_text(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    // SICHERHEIT: Nur lesen/schreiben, niemals System-Dateien ändern
    // use std::fs::OpenOptions;
    //use std::io::{Seek, SeekFrom, Write};

    // Versuche Framebuffer zu öffnen (read-only erst)
    let fb_path = "/dev/fb0";

    // Test: Können wir überhaupt auf den Framebuffer zugreifen?
    if !std::path::Path::new(fb_path).exists() {
        return Err("Framebuffer nicht verfügbar".into());
    }

    // Nur für Debug: Text in Console ausgeben
    println!("Framebuffer verfügbar - zeige Text: {}", text);

    // SICHERHEIT: Vorerst nur simulieren, nicht wirklich schreiben
    // Das können wir später aktivieren wenn wir sicher sind
    /*
    let mut file = OpenOptions::new()
        .write(true)
        .open(fb_path)?;

    // Sehr einfach: Ein paar weiße Pixel in der rechten oberen Ecke
    let start_pos = (50 * 1072 + 800) as u64; // Y=50, X=800 für 1072px breites Display

    file.seek(SeekFrom::Start(start_pos))?;

    // Einfaches "Rechteck" aus weißen Pixeln (8x8)
    for row in 0..8 {
        for _col in 0..8 {
            file.write_all(&[255u8])?; // Weiß
        }
        file.seek(SeekFrom::Current(1072 - 8))?; // Zur nächsten Zeile
    }
    */

    Ok(())
}

fn clear_display() -> Result<(), Box<dyn std::error::Error>> {
    // Versuche das Display zu refreshen/löschen
    let _output = Command::new("eips")
        .args(&["-c"]) // Clear screen
        .output();

    // Alternative: Vollbild-Refresh
    let _output2 = Command::new("eips")
        .args(&["-f"]) // Full refresh
        .output();

    println!("Display-Clear Befehle gesendet");

    Ok(())
}
