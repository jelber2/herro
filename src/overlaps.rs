use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;

use std::fmt;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use crate::aligners::{cigar_to_string, CigarOp};
use crate::haec_io::HAECRecord;

const OL_THRESHOLD: u32 = 2500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strand {
    Forward,
    Reverse,
}

impl fmt::Display for Strand {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            Self::Forward => '+',
            Self::Reverse => '-',
        };

        write!(f, "{}", s)
    }
}

#[derive(Debug)]
pub struct Overlap {
    pub qid: u32,
    pub qlen: u32,
    pub qstart: u32,
    pub qend: u32,
    pub strand: Strand,
    pub tid: u32,
    pub tlen: u32,
    pub tstart: u32,
    pub tend: u32,
    pub cigar: Option<Vec<CigarOp>>,
}

impl Overlap {
    pub fn new(
        qid: u32,
        qlen: u32,
        qstart: u32,
        qend: u32,
        strand: Strand,
        tid: u32,
        tlen: u32,
        tstart: u32,
        tend: u32,
    ) -> Self {
        Overlap {
            qid,
            qlen,
            qstart,
            qend,
            strand,
            tid,
            tlen,
            tstart,
            tend,
            cigar: None,
        }
    }

    fn target_overlap_length(&self) -> u32 {
        return self.tend - self.tstart;
    }

    pub fn return_other_id(&self, id: u32) -> u32 {
        if self.qid == id {
            return self.tid;
        } else {
            return self.qid;
        }
    }
}

impl PartialEq for Overlap {
    fn eq(&self, other: &Self) -> bool {
        self.qid == other.qid
            && self.qstart == other.qstart
            && self.qend == other.qend
            && self.strand == other.strand
            && self.tid == other.tid
            && self.tstart == other.tstart
            && self.tend == other.tend
    }
}

impl Eq for Overlap {}

pub fn parse_paf<P: AsRef<Path>>(path: P, name_to_id: &HashMap<&str, u32>) -> Vec<Overlap> {
    let file = File::open(path).expect("Cannot open overlap file.");
    let mut reader = BufReader::new(file);

    let mut buffer = String::new();
    let mut overlaps = Vec::new();
    let mut processed = HashSet::default();
    while let Ok(len) = reader.read_line(&mut buffer) {
        if len == 0 {
            break;
        }

        let mut data = buffer[..len - 1].split("\t");

        let qid = match name_to_id.get(data.next().unwrap()) {
            Some(qid) => *qid,
            None => continue,
        };
        let qlen: u32 = data.next().unwrap().parse().unwrap();
        let qstart: u32 = data.next().unwrap().parse().unwrap();
        let qend: u32 = data.next().unwrap().parse().unwrap();

        let strand = match data.next().unwrap() {
            "+" => Strand::Forward,
            "-" => Strand::Reverse,
            _ => panic!("Invalid strand character."),
        };

        let tid = match name_to_id.get(data.next().unwrap()) {
            Some(tid) => *tid,
            None => continue,
        };
        let tlen: u32 = data.next().unwrap().parse().unwrap();
        let tstart: u32 = data.next().unwrap().parse().unwrap();
        let tend: u32 = data.next().unwrap().parse().unwrap();

        buffer.clear();
        if tid == qid {
            // Cannot have self-overlaps
            continue;
        }

        if processed.contains(&(qid, tid)) {
            continue; // We assume the first overlap between two reads is the best one
        }
        processed.insert((qid, tid));

        if is_valid_overlap(qlen, qstart, qend, strand, tlen, tstart, tend) {
            let overlap = Overlap::new(qid, qlen, qstart, qend, strand, tid, tlen, tstart, tend);
            overlaps.push(overlap);
        }
    }

    overlaps.shrink_to_fit();

    eprintln!("Total overlaps {}", overlaps.len());
    overlaps
}

#[allow(dead_code)]
fn find_primary_overlaps(overlaps: &[Overlap]) -> HashSet<usize> {
    let mut ovlps_for_pairs = HashMap::default();
    for i in 0..overlaps.len() {
        ovlps_for_pairs
            .entry((overlaps[i].qid, overlaps[i].tid))
            .or_insert_with(|| Vec::new())
            .push(i);
    }

    let mut kept_overlap_ids = HashSet::default();
    for ((_, _), ovlps) in ovlps_for_pairs {
        let kept_id = match ovlps.len() {
            1 => ovlps[0],
            _ => ovlps
                .into_iter()
                .max_by_key(|id| overlaps[*id].target_overlap_length())
                .unwrap(),
        };

        kept_overlap_ids.insert(kept_id);
    }

    kept_overlap_ids
}

fn is_valid_overlap(
    qlen: u32,
    qstart: u32,
    qend: u32,
    strand: Strand,
    tlen: u32,
    tstart: u32,
    tend: u32,
) -> bool {
    let ratio = (tend - tstart) as f64 / (qend - qstart) as f64;
    if ratio < 0.9 || ratio > 1.111 {
        return false;
    }

    if (qlen - (qend - qstart)) <= OL_THRESHOLD {
        return true;
    }

    // Target contained in query
    if (tlen - (tend - tstart)) <= OL_THRESHOLD {
        return true;
    }

    let (qstart, qend) = match strand {
        Strand::Forward => (qstart, qend),
        Strand::Reverse => (qlen - qend, qlen - qstart),
    };

    // Prefix overlap between query and target
    if qstart > OL_THRESHOLD && tstart <= OL_THRESHOLD && (qlen - qend) <= OL_THRESHOLD {
        return true;
    }

    // Suffix overlap between query and target
    if tstart > OL_THRESHOLD && qstart <= OL_THRESHOLD && (tlen - tend) <= OL_THRESHOLD {
        return true;
    }

    false
}

pub fn extend_overlaps(overlaps: &mut [Overlap]) {
    //let primary_overlaps = find_primary_overlaps(&overlaps);
    //println!("Number of primary overlaps {}", primary_overlaps.len());

    /*overlaps
    .into_iter()
    .enumerate()
    //.filter(|(i, _)| primary_overlaps.contains(i)) // Keep only primary overlaps
    .map(|(_, o)| {
        //extend_overlap(&mut o);
        o
    })
    .filter(|o| {
        let b = is_valid_overlap(o);
        b
    })
    .collect()*/

    overlaps.iter_mut().for_each(|mut o| match o.strand {
        Strand::Forward => {
            let beginning = o.tstart.min(o.qstart).min(2500);
            o.tstart -= beginning;
            o.qstart -= beginning;
            let end = (o.tlen - o.tend).min(o.qlen - o.qend).min(2500);
            o.tend += end;
            o.qend += end;
        }
        Strand::Reverse => {
            let beginning = o.tstart.min(o.qlen - o.qend).min(2500);
            o.tstart -= beginning;
            o.qend += beginning;

            let end = (o.tlen - o.tend).min(o.qstart).min(2500);
            o.tend += end;
            o.qstart -= end;
        }
    });
}

#[allow(dead_code)]
pub(crate) fn print_overlaps(overlaps: &[Overlap], reads: &[HAECRecord]) {
    for overlap in overlaps {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            reads[overlap.qid as usize].id,
            overlap.qlen,
            overlap.qstart,
            overlap.qend,
            overlap.strand,
            reads[overlap.tid as usize].id,
            overlap.tlen,
            overlap.tstart,
            overlap.tend,
            match overlap.cigar {
                Some(ref c) => cigar_to_string(c),
                None => "".to_string(),
            }
        )
    }
}
