#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

use std::{
    env,
    fs::File,
    io::{self, BufReader, BufWriter, Write},
    path::Path,
};

use openpaintr::{Document, Pixel};

#[cfg(feature = "gui")]
mod gui;

const USAGE: &str = "Usage: openpaintr [gui]\n       openpaintr demo <file.pam>\n       openpaintr demo-png <file.png>\n       openpaintr png <input.png> <output.png>\n       openpaintr pdn <input.pdn> <output.png>\n       openpaintr --help";

fn main() -> io::Result<()> {
    let mut args = env::args_os().skip(1);
    match (args.next(), args.next(), args.next(), args.next()) {
        (Some(command), Some(path), None, None) if command == "demo" => {
            let document = demo()?;
            let mut output = new_output(Path::new(&path))?;
            document.write_pam(&mut output)?;
            output.flush()
        }
        (Some(command), Some(path), None, None) if command == "demo-png" => {
            export_png(&demo()?, Path::new(&path))
        }
        (Some(command), Some(input), Some(output), None) if command == "png" => {
            let document = Document::read_png(BufReader::new(File::open(input)?))?;
            export_png(&document, Path::new(&output))
        }
        (Some(command), Some(input), Some(output), None) if command == "pdn" => {
            let document = Document::read_pdn(File::open(input)?)?;
            export_png(&document, Path::new(&output))
        }
        #[cfg(feature = "gui")]
        (None, None, None, None) => gui::run(),
        #[cfg(feature = "gui")]
        (Some(command), None, None, None) if command == "gui" => gui::run(),
        #[cfg(not(feature = "gui"))]
        (None, None, None, None) => {
            println!("{USAGE}");
            Ok(())
        }
        (Some(command), None, None, None) if command == "--help" || command == "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, USAGE)),
    }
}

fn new_output(path: &Path) -> io::Result<BufWriter<File>> {
    Ok(BufWriter::new(File::create_new(path)?))
}

fn export_png(document: &Document, path: &Path) -> io::Result<()> {
    let mut output = new_output(path)?;
    document.write_png(&mut output)?;
    output.flush()
}

fn demo() -> io::Result<Document> {
    let mut document = Document::new(320, 240)?;
    let blue = document.add_layer("Azul")?;
    for y in 30..175 {
        for x in 40..200 {
            document.set_pixel(blue, x, y, Pixel([32, 104, 216, 255]))?;
        }
    }
    let orange = document.add_layer("Laranja")?;
    document.set_layer_properties(orange, true, 165)?;
    for y in 75..210 {
        for x in 120..280 {
            document.set_pixel(orange, x, y, Pixel([240, 124, 48, 255]))?;
        }
    }
    Ok(document)
}
