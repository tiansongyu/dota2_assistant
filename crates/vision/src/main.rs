#[cfg(feature = "opencv-backend")]
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(feature = "opencv-backend")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 || !["inspect", "evaluate"].contains(&args[1].as_str()) {
        return Err("usage: dpda-vision inspect PROFILE.json TEMPLATES.json SCREENSHOT.png\n       dpda-vision evaluate PROFILE.json TEMPLATES.json DATASET.json".into());
    }
    if args[1] == "evaluate" {
        let report = dpda_vision::evaluation::evaluate(&args[2], &args[3], &args[4])?;
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        let mut recognizer = dpda_vision::Recognizer::from_files(&args[2], &args[3])?;
        let frame = dpda_vision::read_image_frame(&args[4], recognizer.profile().dpi, 1)?;
        let analysis = recognizer.analyze_frame(&frame)?;
        println!("{}", serde_json::to_string_pretty(&analysis)?);
    }
    Ok(())
}

#[cfg(not(feature = "opencv-backend"))]
fn main() {
    eprintln!("请启用 opencv-backend feature");
    std::process::exit(1);
}
