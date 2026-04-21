// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::io::Read;

use ini::Ini;
use ouroboros::self_referencing;

pub enum EntryPoint<'a> {
    Module(&'a str),
    Callable {
        module: &'a str,
        attribute_chain: &'a str,
    },
}

impl<'a> EntryPoint<'a> {
    fn parse(value: &'a str) -> Self {
        let mut components = value.splitn(2, ":");
        let module = components
            .next()
            .expect("A split always yield at least one item.");
        if let Some(attribute_chain) = components.next()
            && !attribute_chain.is_empty()
        {
            Self::Callable {
                module,
                attribute_chain,
            }
        } else {
            Self::Module(module)
        }
    }
}

impl<'a> Display for EntryPoint<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            EntryPoint::Module(module) => write!(f, "{module}"),
            EntryPoint::Callable {
                module,
                attribute_chain,
            } => write!(f, "{module}:{attribute_chain}"),
        }
    }
}

#[self_referencing]
pub struct EntryPoints {
    contents: Ini,

    #[borrows(contents)]
    #[covariant]
    console_scripts: HashMap<&'this str, EntryPoint<'this>>,

    #[borrows(contents)]
    #[covariant]
    gui_scripts: HashMap<&'this str, EntryPoint<'this>>,
}

impl EntryPoints {
    pub fn empty() -> Self {
        EntryPointsBuilder {
            contents: Ini::new(),
            console_scripts_builder: |_| HashMap::new(),
            gui_scripts_builder: |_| HashMap::new(),
        }
        .build()
    }

    pub fn load(mut contents: impl Read) -> anyhow::Result<Self> {
        Ok(EntryPointsBuilder {
            contents: Ini::read_from(&mut contents)?,
            console_scripts_builder: |contents| parse_entry_points(contents, "console_scripts"),
            gui_scripts_builder: |contents| parse_entry_points(contents, "gui_scripts"),
        }
        .build())
    }

    pub fn is_empty(&self) -> bool {
        self.borrow_console_scripts().is_empty() && self.borrow_gui_scripts().is_empty()
    }

    pub fn is_script(&self, name: impl AsRef<str>) -> bool {
        self.borrow_console_scripts().contains_key(name.as_ref())
            || self.borrow_gui_scripts().contains_key(name.as_ref())
    }

    pub fn console_scripts(&self) -> impl Iterator<Item = (&str, &EntryPoint<'_>)> {
        self.borrow_console_scripts()
            .iter()
            .map(|(name, entry_point)| (*name, entry_point))
    }

    pub fn gui_scripts(&self) -> impl Iterator<Item = (&str, &EntryPoint<'_>)> {
        self.borrow_gui_scripts()
            .iter()
            .map(|(name, entry_point)| (*name, entry_point))
    }
}

fn parse_entry_points<'a>(ini: &'a Ini, section_name: &str) -> HashMap<&'a str, EntryPoint<'a>> {
    ini.section(Some(section_name))
        .map(|section| {
            section
                .iter()
                .map(|(name, entry_point)| (name, EntryPoint::parse(entry_point)))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default()
}
