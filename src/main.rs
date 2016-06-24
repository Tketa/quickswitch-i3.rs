use std::error::Error;
use std::process::{Command, Stdio};

use std::collections::HashMap;

extern crate i3ipc;
use i3ipc::I3Connection;
use i3ipc::reply;

extern crate clap;
use clap::{Arg, App};


static IGNORE_WINDOW_NAME: [&'static str; 1] = ["__i3_scratch"];
static IGNORE_WINDOW_CLASS: [&'static str; 1] = ["i3bar"];

static DEFAULT_DMENU_COMMAND: &'static str = "dmenu -b -i -l 20";

#[derive(Debug)]
struct Window {
    id: i32,
    name: String,
    class_name: Option<String>,
}

#[derive(Debug)]
struct Workspace {
    name: String
}

trait Selectable {
    fn to_select_string(&self) -> String;
}

impl Selectable for Window {
    fn to_select_string(&self) -> String {
        format!("[id=\"{}\"]", self.id)
    }
}

impl Selectable for Workspace {
    fn to_select_string(&self) -> String {
        self.name.to_owned()
    }
}

impl Window {
    fn pad_format(&self, padding: usize) -> String {
        format!("{class: <0$}{name}",
                padding,
                class=self.class_name.as_ref().unwrap_or(&"".to_owned()),
                name=self.name)
    }
}

fn max_class_name_size(windows: &[Window]) -> usize {
    windows.into_iter()
        .map(|w| w.class_name.as_ref().map_or(0, |s| s.len()))
        .max().unwrap()
}

fn split_exec_args(command: &str) -> (String, Vec<String>) {
    use std::fmt::Write;

    let mut iter = command.chars();
    let mut args = Vec::new();

    let mut buf = String::new();

    let mut skip = false;
    let mut matching_char: Option<char> = None;

    while let Some(ch) = iter.next() {
        if skip {
            skip = false;
            continue;
        }
        match matching_char {
            Some(mc) => {
                match ch {
                    '"' | '\'' => if mc == ch {
                        args.push(buf.to_owned());
                        buf = String::new();
                        matching_char = None;
                    } else {
                        let b = &mut buf;
                        b.write_char(ch).unwrap();
                    },
                    _ => {
                        let b = &mut buf;
                        b.write_char(ch).unwrap();
                    },
                }
            }
            None => {
                match ch {
                    ' ' => {
                        args.push(buf.to_owned());
                        buf = String::new();
                    }
                    '"' | '\'' => matching_char = Some(ch),
                    '\\' => skip = true,
                    _ => {
                        let b = &mut buf;
                        b.write_char(ch).unwrap();
                    },
                }
            }
        }
    }

    let program = args.remove(0);

    (program, args)
}

fn get_windows_names(conn: &mut I3Connection) -> Vec<Window> {
    let nodes = conn.get_tree().unwrap().nodes;
    let flatten_nodes = flatten_nodes(&nodes);

    flatten_nodes.into_iter().filter(|n| filter_node(n)).flat_map(|m| {
        match m.name {
            Some(ref name) => {
                vec![Window {
                    id: m.window.unwrap(),
                    name: name.to_owned(),
                    class_name: m.class_name.to_owned()
                }]
            },
            None => vec![]
        }
    }).collect::<Vec<_>>()
}

fn filter_node(node: &reply::Node) -> bool {
    // if not, it isn't a x window
    node.window.is_some() &&
    match node.name {
        Some(ref name) => !IGNORE_WINDOW_NAME.contains(&name.as_str()),
        None => false // ignore window without a name ?
    } &&
    match node.class_name {
        Some(ref name) => !IGNORE_WINDOW_CLASS.contains(&name.as_str()),
        None => true
    }
}

fn flatten_nodes(nodes: &[reply::Node]) -> Vec<&reply::Node> {
    nodes.into_iter().flat_map(|n| {
        if !n.nodes.is_empty() {
            flatten_nodes(&n.nodes)
        } else {
            vec![n]
        }
    }).collect::<Vec<_>>()
}

// [TODO]: Fix args splitting for subcommand - 2016-06-24 10:43
// Currently, it simply split it at whitespace, which is wrong.
fn exec_dmenu(exec: &str, options: &str) -> String {
    use std::io::prelude::*;
    let (program, args) = split_exec_args(exec);
    println!("{} | {:?}", program, args);
    let cmd = Command::new(program)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    match cmd.stdin.unwrap().write_all(options.as_bytes()) {
        Err(why) => panic!("{}", why.description()),
        Ok(_) => (),
    }

    let mut s = String::new();
    match cmd.stdout.unwrap().read_to_string(&mut s) {
        _ => ()
    }
    s
}

fn main() {
    let matches = App::new("Quickswitch-i3.rs")
        .version("0.1")
        .author("Jocelyn B. <kazoomy@gmail.com>")
        .arg(Arg::with_name("dmenu")
             .short("d")
             .long("dmenu")
             .value_name("DMENU")
             .help("dmenu command to execute")
             .takes_value(true))
        .arg(Arg::with_name("move")
             .short("m")
             .long("move"))
        .arg(Arg::with_name("workspace")
             .short("w")
             .long("workspace"))
        .get_matches();

    let dmenu_command = matches.value_of("dmenu").unwrap_or(DEFAULT_DMENU_COMMAND);
    println!("{:?}", dmenu_command);

    // if !matches.is_present("move") {
    //     panic!("Not implemented");
    // }

    let mut connection = I3Connection::connect().unwrap();

    let mut mapping: HashMap<String, Box<Selectable>> = HashMap::new();
    if matches.is_present("workspace") {
        let workspaces = connection.get_workspaces().unwrap().workspaces;

        for w in workspaces {
            let workspace = Workspace { name: w.name.to_owned() };
            mapping.insert(w.name, Box::new(workspace));
        }

    } else if matches.is_present("move") {
        let windows = get_windows_names(&mut connection);
        let max_cname_size = max_class_name_size(&windows) + 5;

        for w in windows {
            mapping.insert(w.pad_format(max_cname_size), Box::new(w));
        }

    }

    let options = mapping.keys().map(|s| s.to_string()).collect::<Vec<_>>().as_slice().join("\n");
    let str_result = exec_dmenu(&dmenu_command, &options);

    if matches.is_present("workspace") {
        let trimmed = str_result.trim();
        let res = match mapping.get(trimmed) {
            Some(win) => win.to_select_string(),
            None => trimmed.to_owned(),
        };
        connection.command(&format!("workspace {}", res));

    } else if matches.is_present("move") {
        if let Some(res) = mapping.get(str_result.trim()) {
            let res = connection.command(&format!("{} move workspace current", res.to_select_string()));
            println!("{:?}", res)
        }
    }
}
