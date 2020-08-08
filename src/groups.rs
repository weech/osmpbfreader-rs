// Copyright (c) 2014-2015 Guillaume Pinot <texitoi(a)texitoi.eu>
//
// This work is free. You can redistribute it and/or modify it under
// the terms of the Do What The Fuck You Want To Public License,
// Version 2, as published by Sam Hocevar. See the COPYING file for
// more details.

use objects::*;
use osmformat;
use osmformat::{PrimitiveBlock, PrimitiveGroup};
use smartstring::alias::String;
use std::borrow::Cow;
use std::convert::From;
use std::iter::Chain;
use std::iter::Map;
use std::slice;

pub_iterator_type! {
    #[doc="Iterator on the `OsmObj` of a `PrimitiveGroup`."]
    OsmObjs['a] = Chain<Chain<Map<Nodes<'a>, fn(Node) -> OsmObj>,
                              Map<Ways<'a>, fn(Way) -> OsmObj>>,
                        Map<Relations<'a>, fn(Relation) -> OsmObj>>
}

pub fn iter<'a>(g: &'a PrimitiveGroup, b: &'a PrimitiveBlock) -> OsmObjs<'a> {
    let iter = nodes(g, b)
        .map(From::from as fn(Node) -> OsmObj)
        .chain(ways(g, b).map(From::from as fn(Way) -> OsmObj))
        .chain(relations(g, b).map(From::from as fn(Relation) -> OsmObj));
    OsmObjs(iter)
}

pub_iterator_type! {
    #[doc="Iterator on the `Node` of a `PrimitiveGroup`."]
    Nodes['a] = Chain<SimpleNodes<'a>, DenseNodes<'a>>
}

pub fn nodes<'a>(g: &'a PrimitiveGroup, b: &'a PrimitiveBlock) -> Nodes<'a> {
    Nodes(simple_nodes(g, b).chain(dense_nodes(g, b)))
}

pub fn simple_nodes<'a>(group: &'a PrimitiveGroup, block: &'a PrimitiveBlock) -> SimpleNodes<'a> {
    SimpleNodes {
        iter: group.get_nodes().iter(),
        block: block,
    }
}

pub struct SimpleNodes<'a> {
    iter: slice::Iter<'a, osmformat::Node>,
    block: &'a PrimitiveBlock,
}

impl<'a> Iterator for SimpleNodes<'a> {
    type Item = Node;
    fn next(&mut self) -> Option<Node> {
        self.iter.next().map(|n| Node {
            id: NodeId(n.get_id()),
            decimicro_lat: make_lat(n.get_lat(), self.block),
            decimicro_lon: make_lon(n.get_lon(), self.block),
            tags: make_tags(n.get_keys(), n.get_vals(), self.block),
            info: if n.has_info() {
                make_info(n.get_info(), self.block)
            } else {
                Info {
                    version: None,
                    timestamp: None,
                    changeset: None,
                    uid: None,
                    user: None,
                    visible: None,
                }
            },
        })
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

struct DenseInfoIter<'a> {
    denseinfo: &'a protobuf::SingularPtrField<osmformat::DenseInfo>,
    block: &'a osmformat::PrimitiveBlock,
    cur_timestamp: i64,
    cur_changeset: i64,
    cur_uid: i32,
    cur_user_sid: i32,
    place: usize,
}

impl<'a> DenseInfoIter<'a> {
    fn new(
        denseinfo: &'a protobuf::SingularPtrField<osmformat::DenseInfo>,
        block: &'a osmformat::PrimitiveBlock,
    ) -> Self {
        Self {
            denseinfo,
            block,
            cur_timestamp: 0,
            cur_changeset: 0,
            cur_uid: 0,
            cur_user_sid: 0,
            place: 0,
        }
    }
}

impl<'a> Iterator for DenseInfoIter<'a> {
    type Item = Info;
    fn next(&mut self) -> Option<Self::Item> {
        if self.denseinfo.is_some() {
            let di = self.denseinfo.get_ref();
            if self.place == di.version.len() {
                return None;
            }
            let version = di.version[self.place];
            self.cur_timestamp += di.timestamp[self.place];
            self.cur_changeset += di.changeset[self.place];
            self.cur_uid += di.uid[self.place];
            self.cur_user_sid += di.user_sid[self.place];
            let visible = di.visible[self.place];
            self.place += 1;
            Some(Info {
                version: Some(version),
                timestamp: Some(self.cur_timestamp),
                changeset: Some(self.cur_changeset),
                uid: Some(self.cur_uid),
                user: Some(make_string(self.cur_user_sid as usize, self.block)),
                visible: Some(visible),
            })
        } else {
            Some(Info {
                version: None,
                timestamp: None,
                changeset: None,
                uid: None,
                user: None,
                visible: None,
            })
        }
    }
}

pub fn dense_nodes<'a>(group: &'a PrimitiveGroup, block: &'a PrimitiveBlock) -> DenseNodes<'a> {
    let dense = group.get_dense();
    DenseNodes {
        block: block,
        dids: dense.get_id().iter(),
        dlats: dense.get_lat().iter(),
        dlons: dense.get_lon().iter(),
        keys_vals: dense.get_keys_vals().iter(),
        cur_id: 0,
        cur_lat: 0,
        cur_lon: 0,
        denseinfo: DenseInfoIter::new(&dense.denseinfo, block),
    }
}

pub struct DenseNodes<'a> {
    block: &'a PrimitiveBlock,
    dids: slice::Iter<'a, i64>,
    dlats: slice::Iter<'a, i64>,
    dlons: slice::Iter<'a, i64>,
    keys_vals: slice::Iter<'a, i32>,
    cur_id: i64,
    cur_lat: i64,
    cur_lon: i64,
    denseinfo: DenseInfoIter<'a>,
}

impl<'a> Iterator for DenseNodes<'a> {
    type Item = Node;
    fn next(&mut self) -> Option<Node> {
        let info = match (
            self.dids.next(),
            self.dlats.next(),
            self.dlons.next(),
            self.denseinfo.next(),
        ) {
            (Some(&did), Some(&dlat), Some(&dlon), Some(info)) => {
                self.cur_id += did;
                self.cur_lat += dlat;
                self.cur_lon += dlon;
                info
            }
            _ => return None,
        };
        let mut tags = Tags::new();
        loop {
            let k = match self.keys_vals.next() {
                None | Some(&0) => break,
                Some(k) => make_string(*k as usize, self.block),
            };
            let v = match self.keys_vals.next() {
                None => break,
                Some(v) => make_string(*v as usize, self.block),
            };
            tags.insert(k, v);
        }
        tags.shrink_to_fit();
        Some(Node {
            id: NodeId(self.cur_id),
            decimicro_lat: make_lat(self.cur_lat, self.block),
            decimicro_lon: make_lon(self.cur_lon, self.block),
            tags: tags,
            info,
        })
    }
}

pub fn ways<'a>(group: &'a PrimitiveGroup, block: &'a PrimitiveBlock) -> Ways<'a> {
    Ways {
        iter: group.get_ways().iter(),
        block: block,
    }
}

pub struct Ways<'a> {
    iter: slice::Iter<'a, osmformat::Way>,
    block: &'a PrimitiveBlock,
}

impl<'a> Iterator for Ways<'a> {
    type Item = Way;
    fn next(&mut self) -> Option<Way> {
        self.iter.next().map(|w| {
            let mut n = 0;
            let nodes = w
                .get_refs()
                .iter()
                .map(|&dn| {
                    n += dn;
                    NodeId(n)
                })
                .collect();
            Way {
                id: WayId(w.get_id()),
                nodes: nodes,
                tags: make_tags(w.get_keys(), w.get_vals(), self.block),
                info: if w.has_info() {
                    make_info(w.get_info(), self.block)
                } else {
                    Info {
                        version: None,
                        timestamp: None,
                        changeset: None,
                        uid: None,
                        user: None,
                        visible: None,
                    }
                },
            }
        })
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

pub fn relations<'a>(group: &'a PrimitiveGroup, block: &'a PrimitiveBlock) -> Relations<'a> {
    Relations {
        iter: group.get_relations().iter(),
        block: block,
    }
}
pub struct Relations<'a> {
    iter: slice::Iter<'a, osmformat::Relation>,
    block: &'a PrimitiveBlock,
}

impl<'a> Iterator for Relations<'a> {
    type Item = Relation;
    fn next(&mut self) -> Option<Relation> {
        use osmformat::Relation_MemberType::{NODE, RELATION, WAY};
        self.iter.next().map(|rel| {
            let mut m = 0;
            let refs = rel
                .get_memids()
                .iter()
                .zip(rel.get_types().iter())
                .zip(rel.get_roles_sid().iter())
                .map(|((&dm, &t), &role)| {
                    m += dm;
                    Ref {
                        member: match t {
                            NODE => NodeId(m).into(),
                            WAY => WayId(m).into(),
                            RELATION => RelationId(m).into(),
                        },
                        role: make_string(role as usize, self.block),
                    }
                })
                .collect();
            Relation {
                id: RelationId(rel.get_id()),
                refs: refs,
                tags: make_tags(rel.get_keys(), rel.get_vals(), self.block),
                info: if rel.has_info() {
                    make_info(rel.get_info(), self.block)
                } else {
                    Info {
                        version: None,
                        timestamp: None,
                        changeset: None,
                        uid: None,
                        user: None,
                        visible: None,
                    }
                },
            }
        })
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

fn make_string(k: usize, block: &osmformat::PrimitiveBlock) -> String {
    let cow = std::string::String::from_utf8_lossy(&*block.get_stringtable().get_s()[k]);
    match cow {
        Cow::Borrowed(s) => String::from(s),
        Cow::Owned(s) => String::from(s),
    }
}

fn make_lat(c: i64, b: &osmformat::PrimitiveBlock) -> i32 {
    let granularity = b.get_granularity() as i64;
    ((b.get_lat_offset() + granularity * c) / 100) as i32
}

fn make_lon(c: i64, b: &osmformat::PrimitiveBlock) -> i32 {
    let granularity = b.get_granularity() as i64;
    ((b.get_lon_offset() + granularity * c) / 100) as i32
}

fn make_tags(keys: &[u32], vals: &[u32], b: &PrimitiveBlock) -> Tags {
    let mut tags: Tags = keys
        .iter()
        .zip(vals.iter())
        .map(|(&k, &v)| (make_string(k as usize, b), make_string(v as usize, b)))
        .collect();
    tags.shrink_to_fit();
    tags
}

fn make_info(i: &osmformat::Info, b: &PrimitiveBlock) -> Info {
    let version = if i.has_version() {
        Some(i.get_version())
    } else {
        None
    };
    let timestamp = if i.has_timestamp() {
        Some(i.get_timestamp())
    } else {
        None
    };
    let changeset = if i.has_changeset() {
        Some(i.get_changeset())
    } else {
        None
    };
    let uid = if i.has_uid() { Some(i.get_uid()) } else { None };
    let user = if i.has_user_sid() {
        Some(make_string(i.get_user_sid() as usize, b))
    } else {
        None
    };
    let visible = if i.has_visible() {
        Some(i.get_visible())
    } else {
        None
    };
    Info {
        version,
        timestamp,
        changeset,
        uid,
        user,
        visible,
    }
}
