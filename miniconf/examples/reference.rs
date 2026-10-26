//! Export settings and construction defaults for miniconf-doc.

use miniconf::{Tree, json::to_json_value, json_schema::TreeJsonSchema};

#[derive(Tree)]
#[tree(meta(typename, doc))]
/// Output channel configuration.
struct Channel {
    #[tree(meta(doc, unit = "A/V"))]
    /// Conversion gain.
    gain: f32,
}

#[derive(Tree)]
#[tree(meta(typename))]
struct Settings {
    #[tree(meta(doc))]
    /// Main output.
    output: Channel,
    #[tree(meta(doc))]
    /// Monitor output.
    monitor: Channel,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Settings {
        output: Channel { gain: 1.0 },
        monitor: Channel { gain: 0.5 },
    };
    let mut schema = TreeJsonSchema::new(Some(&settings))?.root;
    schema.insert("title".into(), "Current driver".into());
    schema.insert("default".into(), to_json_value(&settings)?);
    println!("{}", serde_json::to_string_pretty(&schema)?);
    Ok(())
}
