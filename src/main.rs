mod protos;

use crate::protos::profile;
use protobuf::Message;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol {
    pub name: Option<String>,
    pub file: Option<String>,
}

impl Symbol {
    pub fn name(&self) -> String {
        self.name.clone().unwrap_or("<Unknown>".to_owned())
    }

    pub fn file(&self) -> String {
        self.file.clone().unwrap_or("<Unknown>".to_owned())
    }
}

struct Frame {
    stack: Vec<Symbol>,
    cycles: u64,
}

const SAMPLES: &str = "samples";
const COUNT: &str = "count";
const CPU: &str = "cpu";
const NANOSECONDS: &str = "nanoseconds";

// TODO: make this a CLI argument, right now it's set as 1Ghz, meaning
// 1 CKB cycle takes 1 nanosecond to run.
const FREQUENCY: u64 = 1_000_000_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut frames = Vec::new();

    for line in std::io::stdin().lines() {
        let line = line?;
        let i = line.rfind(" ").expect("no cycles available!");

        let mut stack: Vec<Symbol> = line[0..i]
            .split("; ")
            .map(|s| match s.find(":") {
                Some(j) => Symbol {
                    file: Some(s[0..j].to_string()),
                    name: Some(normalize_function_name(&s[j + 1..s.len()])),
                },
                None => Symbol {
                    name: Some(normalize_function_name(s)),
                    file: None,
                },
            })
            .collect();
        stack.reverse();
        let cycles = u64::from_str(&line[i + 1..line.len()]).expect("invalid cycle");

        frames.push(Frame { stack, cycles });
    }

    let mut dedup_str: HashSet<String> = HashSet::new();
    for Frame { stack, .. } in &frames {
        for symbol in stack {
            dedup_str.insert(symbol.name());
            dedup_str.insert(symbol.file());
        }
    }

    dedup_str.insert(SAMPLES.into());
    dedup_str.insert(COUNT.into());
    dedup_str.insert(CPU.into());
    dedup_str.insert(NANOSECONDS.into());

    // string table's first element must be an empty string
    let mut str_tbl = vec!["".to_owned()];
    str_tbl.extend(dedup_str.into_iter());

    let mut strings = HashMap::new();
    for (index, name) in str_tbl.iter().enumerate() {
        strings.insert(name.clone(), index);
    }

    let mut samples = vec![];
    let mut loc_tbl = vec![];
    let mut fn_tbl = vec![];
    let mut functions = HashMap::new();
    for Frame { stack, cycles } in &frames {
        let mut locs = vec![];
        for symbol in stack {
            let name = symbol.name();
            if let Some(loc_idx) = functions.get(&name) {
                locs.push(*loc_idx);
                continue;
            }
            let function_id = fn_tbl.len() as u64 + 1;
            let function = profile::Function {
                id: function_id,
                name: strings[&name] as i64,
                // TODO: distinguish between C++ mangled & unmangled names
                system_name: strings[&name] as i64,
                filename: strings[&symbol.file()] as i64,
                ..Default::default()
            };
            functions.insert(name, function_id);
            let line = profile::Line {
                function_id,
                line: 0,
                ..Default::default()
            };
            let loc = profile::Location {
                id: function_id,
                line: vec![line].into(),
                ..Default::default()
            };
            // the fn_tbl has the same length with loc_tbl
            fn_tbl.push(function);
            loc_tbl.push(loc);
            // current frame locations
            locs.push(function_id);
        }
        let sample = profile::Sample {
            location_id: locs,
            value: vec![
                *cycles as i64,
                *cycles as i64 * 1_000_000_000 / FREQUENCY as i64,
            ],
            label: vec![].into(),
            ..Default::default()
        };
        samples.push(sample);
    }
    let samples_value = profile::ValueType {
        field_type: strings[SAMPLES] as i64,
        unit: strings[COUNT] as i64,
        ..Default::default()
    };
    let time_value = profile::ValueType {
        field_type: strings[CPU] as i64,
        unit: strings[NANOSECONDS] as i64,
        ..Default::default()
    };
    let profile = profile::Profile {
        sample_type: vec![samples_value, time_value.clone()].into(),
        sample: samples.into(),
        string_table: str_tbl.into(),
        function: fn_tbl.into(),
        location: loc_tbl.into(),
        period_type: Some(time_value).into(),
        period: 1_000_000_000 / FREQUENCY as i64,
        ..Default::default()
    };
    let data = profile.write_to_bytes().expect("protobuf serialization");
    std::fs::write("output.pprof", data)?;

    Ok(())
}

fn normalize_function_name(name: &str) -> String {
    name.replace("<", "{").replace(">", "}").to_string()
}
