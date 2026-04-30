// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use anyhow::anyhow;
use cache::{Fingerprint, default_digest, fingerprint_file};
use dashmap::DashMap;

#[derive(Debug)]
pub struct Provenance {
    subject: String,
    origins: DashMap<PathBuf, String>,
    collisions: DashMap<PathBuf, Vec<(String, Fingerprint, usize)>>,
}

impl Provenance {
    pub fn new(subject: String) -> Self {
        Self {
            subject,
            origins: DashMap::new(),
            collisions: DashMap::new(),
        }
    }

    pub(crate) fn record(&self, src: impl Display, dst: PathBuf) {
        let existing = self.origins.insert(dst, src.to_string());
        assert!(
            existing.is_none(),
            "The record method should only be called upon successful creation of a new file."
        );
    }

    pub(crate) fn record_collision(
        &self,
        src: impl Display,
        fingerprint: Fingerprint,
        size: usize,
        dst: PathBuf,
    ) {
        self.collisions
            .entry(dst)
            .or_default()
            .push((src.to_string(), fingerprint, size))
    }

    pub fn into_collision_report(self) -> anyhow::Result<Option<CollisionReport>> {
        if self.collisions.is_empty() {
            return Ok(None);
        }
        let mut collisions = Vec::with_capacity(self.collisions.len());
        for (dst, mut collision_details) in self.collisions {
            let (_, source) = self.origins.remove(&dst).ok_or_else(|| {
                anyhow!(
                    "A collision always has a corresponding origin but {dst} has none",
                    dst = dst.display()
                )
            })?;

            let mut srcs: BTreeMap<(Fingerprint, usize), Vec<String>> = BTreeMap::new();
            let (size, fingerprint) = fingerprint_file(&dst, default_digest())?;
            srcs.entry((fingerprint, size)).or_default().push(source);
            collision_details.sort_by(|(source1, _, _), (source2, _, _)| source1.cmp(source2));
            for (source, fingerprint, size) in collision_details {
                srcs.entry((fingerprint, size)).or_default().push(source);
            }
            if srcs.len() > 1 {
                collisions.push(Collision { dst, srcs })
            }
        }
        if collisions.is_empty() {
            return Ok(None);
        }
        collisions.sort_by(|left, right| left.dst.cmp(&right.dst));
        Ok(Some(CollisionReport {
            subject: self.subject,
            collisions,
        }))
    }
}

pub struct Collision {
    dst: PathBuf,
    srcs: BTreeMap<(Fingerprint, usize), Vec<String>>,
}

impl Display for Collision {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Had {count} distinct sources for {dst}:",
            count = self.srcs.len(),
            dst = self.dst.display()
        )?;
        for (index, ((fingerprint, size), srcs)) in self.srcs.iter().enumerate() {
            writeln!(
                f,
                "{idx}. {fingerprint} {size} bytes",
                idx = index + 1,
                fingerprint = fingerprint.hex_digest()
            )?;
            for src in srcs {
                writeln!(f, "    {src}")?;
            }
        }
        Ok(())
    }
}

pub struct CollisionReport {
    subject: String,
    collisions: Vec<Collision>,
}

impl Display for CollisionReport {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let count = self.collisions.len();
        writeln!(
            f,
            "While {subject} encountered {count} {collisions}:",
            subject = self.subject,
            collisions = if count == 1 {
                "collision"
            } else {
                "collisions"
            }
        )?;
        writeln!(f)?;
        for collision in &self.collisions {
            writeln!(f, "{collision}")?;
        }
        Ok(())
    }
}
