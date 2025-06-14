// Copy-pasted from
// [here](https://github.com/posborne/rust-pstree/blob/2ef62f0e2d05b95b68c321de2bcb3d3cf16f20b3/pstree.rs)
use std::path::Path;
use std::fs;
use std::io::prelude::*;
use std::fs::File;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::collections::HashMap;
use crate::error::Result;

#[derive(Clone,Debug)]
pub struct ProcessRecord {
    pub pid: i32,
    pub ppid: i32,
}

#[derive(Clone,Debug)]
pub struct ProcessTreeNode {
    pub record: ProcessRecord,  // the node owns the associated record
    pub children: Vec<ProcessTreeNode>, // nodes own their children
}

#[derive(Clone,Debug)]
pub struct ProcessTree {
    pub root: ProcessTreeNode, // tree owns ref to root node
}

impl ProcessTreeNode {
    // constructor
    pub fn new(record : &ProcessRecord) -> ProcessTreeNode {
        ProcessTreeNode { record: (*record).clone(), children: Vec::new() }
    }
}

// Given a status file path, return a hashmap with the following form:
// pid -> ProcessRecord
fn get_process_record(status_path: &Path) -> Option<ProcessRecord> {
    let mut pid : Option<i32> = None;
    let mut ppid : Option<i32> = None;

    let file = File::open(status_path);
    if file.is_err() {
        return None;
    }
    let mut reader = std::io::BufReader::new(file.unwrap());
    loop {
        let mut linebuf = String::new();
        match reader.read_line(&mut linebuf) {
            Ok(_) => {
                if linebuf.is_empty() {
                    break;
                }
                let parts : Vec<&str> = linebuf[..].splitn(2, ':').collect();
                if parts.len() == 2 {
                    let key = parts[0].trim();
                    let value = parts[1].trim();
                    match key {
                        "Pid" => pid = value.parse().ok(),
                        "PPid" => ppid = value.parse().ok(),
                        _ => (),
                    }
                }
            },
            Err(_) => break,
        }
    }
    return if pid.is_some() && ppid.is_some() {
        Some(ProcessRecord { pid: pid.unwrap(), ppid: ppid.unwrap() })
    } else {
        None
    }
}


// build a simple struct (ProcessRecord) for each process
fn get_process_records() -> Result<Vec<ProcessRecord>> {
    let proc_directory = Path::new("/proc");

    // find potential process directories under /proc
    let proc_directory_contents = fs::read_dir(&proc_directory)?;
    fn process_entry(entry: std::io::Result<fs::DirEntry>) -> Result<Option<ProcessRecord>>
    {
        let entry_path = entry?.path();
        if fs::metadata(entry_path.as_path())?.is_dir() {
            let status_path = entry_path.join("status");
            if let Ok(metadata) = fs::metadata(status_path.as_path()) {
                if metadata.is_file() {
                    return Ok(get_process_record(status_path.as_path()));
                }
            }
        }
        Ok(None)
    }
    Ok(proc_directory_contents.filter_map(|entry| {
        process_entry(entry).unwrap_or(None)
    }).collect())
}

fn populate_node_helper(node: &mut ProcessTreeNode, pid_map: &HashMap<i32, &ProcessRecord>, ppid_map: &HashMap<i32, Vec<i32>>) {
    let pid = node.record.pid; // avoid binding node as immutable in closure
    let child_nodes = &mut node.children;
    match ppid_map.get(&pid) {
        Some(children) => {
            child_nodes.extend(children.iter().map(|child_pid| {
                let record = pid_map[child_pid];
                let mut child = ProcessTreeNode::new(record);
                populate_node_helper(&mut child, pid_map, ppid_map);
                child
            }));
        },
        None => {},
    }
}

fn populate_node(node : &mut ProcessTreeNode, records: &Vec<ProcessRecord>) {
    // O(n): build a mapping of pids to vectors of children.  That is, each
    // key is a pid and its value is a vector of the whose parent pid is the key
    let mut ppid_map : HashMap<i32, Vec<i32>> = HashMap::new();
    let mut pid_map : HashMap<i32, &ProcessRecord> = HashMap::new();
    for record in records.iter() {
        // entry returns either a vacant or occupied entry.  If vacant,
        // we insert a new vector with this records pid.  If occupied,
        // we push this record's pid onto the vec
        pid_map.insert(record.pid, record);
        match ppid_map.entry(record.ppid) {
            Vacant(entry) => { entry.insert(vec![record.pid]); },
            Occupied(mut entry) => { entry.get_mut().push(record.pid); },
        };
    }

    // With the data structures built, it is off to the races
    populate_node_helper(node, &pid_map, &ppid_map);
}

pub fn build_process_tree(pid: Option<i32>) -> Result<ProcessTree> {
    let records = get_process_records();
    let mut tree = ProcessTree {
        root : ProcessTreeNode::new(
            &ProcessRecord {
                pid: pid.unwrap_or(0),
                ppid: -1
            })
    };

    // recursively populate all nodes in the tree starting from root (pid 0)
    {
        let root = &mut tree.root;
        populate_node(root, &records?);
    }
    Ok(tree)
}
