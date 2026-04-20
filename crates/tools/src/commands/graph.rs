// Copyright 2026 Pex project contributors.
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Display;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::bail;
use clap::builder::PossibleValue;
use clap::{Args, ValueEnum};
use dot_generator::{attr, edge, id, node, node_id};
use dot_structures::{
    Attribute,
    Edge,
    EdgeTy,
    Graph,
    GraphAttributes,
    Id,
    Node,
    NodeId,
    Stmt,
    Vertex,
};
use fs_err::File;
use graphviz_rust::printer::{DotPrinter, PrinterContext};
use graphviz_rust::{cmd, exec};
use indexmap::IndexMap;
use interpreter::{InterpreterConstraint, SearchPath};
use pep508_rs::{PackageName, Requirement};
use pex::{CollectExtraMetadata, Pex, PexPath};
use url::Url;

use crate::output::Output;

#[derive(Clone)]
struct Format(cmd::Format);

impl Format {
    fn as_command_arg(&self) -> cmd::CommandArg {
        self.0.into()
    }

    fn as_str(&self) -> &'static str {
        match self.0 {
            cmd::Format::Bmp => "bmp",
            cmd::Format::Cgimage => "cgimage",
            cmd::Format::Canon => "cannon",
            cmd::Format::Dot => "dot",
            cmd::Format::Gv => "gv",
            cmd::Format::Xdot => "xdot",
            cmd::Format::Xdot12 => "xdot1.2",
            cmd::Format::Xdot14 => "xdot1.4",
            cmd::Format::Eps => "eps",
            cmd::Format::Exr => "exr",
            cmd::Format::Fig => "fig",
            cmd::Format::Gd => "gd",
            cmd::Format::Gd2 => "gd2",
            cmd::Format::Gif => "gif",
            cmd::Format::Gtk => "gtk",
            cmd::Format::Ico => "ico",
            cmd::Format::Cmap => "cmap",
            cmd::Format::Ismap => "ismap",
            cmd::Format::Imap => "imap",
            cmd::Format::Cmapx => "cmapx",
            cmd::Format::ImapNp => "imap_np",
            cmd::Format::CmapxNp => "cmapx_np",
            cmd::Format::Jpg => "jpg",
            cmd::Format::Jpeg => "jpeg",
            cmd::Format::Jpe => "jpe",
            cmd::Format::Jp2 => "jp2",
            cmd::Format::Json => "json",
            cmd::Format::Json0 => "json0",
            cmd::Format::DotJson => "dot_json",
            cmd::Format::XdotJson => "xdot_json",
            cmd::Format::Pdf => "pdf",
            cmd::Format::Pic => "pic",
            cmd::Format::Pct => "pct",
            cmd::Format::Pict => "pict",
            cmd::Format::Plain => "plain",
            cmd::Format::PlainExt => "plain-ext",
            cmd::Format::Png => "png",
            cmd::Format::Pov => "pov",
            cmd::Format::Ps => "ps",
            cmd::Format::Ps2 => "ps2",
            cmd::Format::Psd => "psd",
            cmd::Format::Sgi => "sgi",
            cmd::Format::Svg => "svg",
            cmd::Format::Svgz => "svgz",
            cmd::Format::Tga => "tga",
            cmd::Format::Tif => "tif",
            cmd::Format::Tiff => "tiff",
            cmd::Format::Tk => "tk",
            cmd::Format::Vml => "vml",
            cmd::Format::Vmlz => "vmlz",
            cmd::Format::Vrml => "vrml",
            cmd::Format::Vbmp => "vbmp",
            cmd::Format::Webp => "webp",
            cmd::Format::Xlib => "xlib",
            cmd::Format::X11 => "x11",
        }
    }
}

impl Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{}", self.as_str())
    }
}

impl ValueEnum for Format {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            Format(cmd::Format::Bmp),
            Format(cmd::Format::Cgimage),
            Format(cmd::Format::Canon),
            Format(cmd::Format::Dot),
            Format(cmd::Format::Gv),
            Format(cmd::Format::Xdot),
            Format(cmd::Format::Xdot12),
            Format(cmd::Format::Xdot14),
            Format(cmd::Format::Eps),
            Format(cmd::Format::Exr),
            Format(cmd::Format::Fig),
            Format(cmd::Format::Gd),
            Format(cmd::Format::Gd2),
            Format(cmd::Format::Gif),
            Format(cmd::Format::Gtk),
            Format(cmd::Format::Ico),
            Format(cmd::Format::Cmap),
            Format(cmd::Format::Ismap),
            Format(cmd::Format::Imap),
            Format(cmd::Format::Cmapx),
            Format(cmd::Format::ImapNp),
            Format(cmd::Format::CmapxNp),
            Format(cmd::Format::Jpg),
            Format(cmd::Format::Jpeg),
            Format(cmd::Format::Jpe),
            Format(cmd::Format::Jp2),
            Format(cmd::Format::Json),
            Format(cmd::Format::Json0),
            Format(cmd::Format::DotJson),
            Format(cmd::Format::XdotJson),
            Format(cmd::Format::Pdf),
            Format(cmd::Format::Pic),
            Format(cmd::Format::Pct),
            Format(cmd::Format::Pict),
            Format(cmd::Format::Plain),
            Format(cmd::Format::PlainExt),
            Format(cmd::Format::Png),
            Format(cmd::Format::Pov),
            Format(cmd::Format::Ps),
            Format(cmd::Format::Ps2),
            Format(cmd::Format::Psd),
            Format(cmd::Format::Sgi),
            Format(cmd::Format::Svg),
            Format(cmd::Format::Svgz),
            Format(cmd::Format::Tga),
            Format(cmd::Format::Tif),
            Format(cmd::Format::Tiff),
            Format(cmd::Format::Tk),
            Format(cmd::Format::Vml),
            Format(cmd::Format::Vmlz),
            Format(cmd::Format::Vrml),
            Format(cmd::Format::Vbmp),
            Format(cmd::Format::Webp),
            Format(cmd::Format::Xlib),
            Format(cmd::Format::X11),
        ]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.as_str()))
    }
}

#[derive(Args)]
pub(crate) struct GraphArgs {
    /// A file to output the dot graph to; STDOUT by default.
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// Attempt to render the graph.
    #[arg(short = 'r', long, default_value_t = false)]
    render: bool,

    /// The format to render the graph in.
    #[arg(short = 'f', long, default_value_t = Format(cmd::Format::Svg))]
    format: Format,

    /// Attempt to open the graph in the system viewer (implies --render).
    #[arg(long, default_value_t = false)]
    open: bool,
}

struct WheelInfo<'a> {
    raw_project_name: &'a str,
    raw_version: &'a str,
    requires_dists: Vec<Requirement<Url>>,
}

pub(crate) fn create(python: &Path, pex: Pex, args: GraphArgs) -> anyhow::Result<()> {
    let search_path = SearchPath::from_env()?;
    let pex_path = PexPath::from_pex_info(&pex.info, true);
    let additional_pexes = pex_path.load_pexes()?;
    let extra_metadata = CollectExtraMetadata::new();
    let resolve = pex.resolve(
        Some(python),
        additional_pexes.iter(),
        search_path,
        Some(extra_metadata.clone()),
    )?;
    let metadata_lookups = extra_metadata.into_lookups()?;

    let mut wheels: IndexMap<PackageName, WheelInfo> = IndexMap::new();
    for wheel in resolve.wheels.values().chain(
        resolve
            .additional_wheels
            .iter()
            .flat_map(|(_, additional_wheels)| additional_wheels.values()),
    ) {
        let wheel_metadata = metadata_lookups
            .for_whl(wheel)
            .expect("Each resolved wheel should be paired with metadata");
        wheels.insert(
            wheel_metadata.project_name.clone(),
            WheelInfo {
                raw_project_name: wheel_metadata.raw_project_name,
                raw_version: wheel_metadata.raw_version,
                requires_dists: wheel_metadata.requires_dists.clone(),
            },
        );
    }

    let mut graph = Graph::DiGraph {
        id: id!(esc pex.path.display()),
        strict: true,
        stmts: Vec::new(),
    };
    let graph_label = format!(
        "Dependency graph of {pex} for interpreter {python_binary} ({python_id})",
        pex = pex.path.display(),
        python_binary = python.display(),
        python_id = InterpreterConstraint::exact_version(&resolve.interpreter)
    );
    graph.add_stmt(Stmt::GAttribute(GraphAttributes::Graph(vec![
        attr!("fontsize", 14),
        attr!("labelloc", esc "t"),
        attr!("label", esc graph_label),
    ])));

    for (project_name, wheel_info) in &wheels {
        let node_label = format!(
            "{name} {version}",
            name = wheel_info.raw_project_name,
            version = wheel_info.raw_version
        );
        let url = format!(
            "https://pypi.org/project/{name}/{version}",
            name = wheel_info.raw_project_name,
            version = wheel_info.raw_version
        );
        graph.add_stmt(Stmt::Node(node!(
            esc project_name;
            attr!("label", esc node_label),
            attr!("URL", esc url),
            attr!("target", esc "_blank")
        )));
        for requirement in &wheel_info.requires_dists {
            if !wheels.contains_key(&requirement.name)
                && !requirement
                    .marker
                    .evaluate(&resolve.interpreter.marker_env, &[])
            {
                let url = format!("https://pypi.org/project/{name}", name = requirement.name);
                graph.add_stmt(Stmt::Node(node!(
                    esc requirement.name;
                    attr!("color", esc "lightgrey"),
                    attr!("style", esc "filled"),
                    attr!("tooltip", esc "inactive requirement"),
                    attr!("URL", esc url),
                    attr!("target", esc "_blank")
                )))
            }
            let mut edge = edge!(
                node_id!(esc project_name) => node_id!(esc requirement.name);
                attr!("fontsize", 10)
            );
            if let Some(label) = match (
                &requirement.version_or_url,
                &requirement.marker.try_to_string(),
            ) {
                (Some(version_or_url), Some(marker)) => Some(format!("{version_or_url}; {marker}")),
                (Some(version_or_url), None) => Some(format!("{version_or_url}")),
                (None, Some(marker)) => Some(format!("; {marker}")),
                _ => None,
            } {
                edge.attributes.push(attr!("label", esc label))
            }
            graph.add_stmt(Stmt::Edge(edge))
        }
    }

    let mut ctx = PrinterContext::default();
    if args.render || args.open {
        let fmt = args.format;
        let rendered = match exec(graph, &mut ctx, vec![fmt.as_command_arg()]) {
            Ok(data) => data,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                bail!(
                    "Do you have `dot` installed on the $PATH?: {err}\n\
                    Find more information on `dot` at https://graphviz.org/."
                )
            }
            Err(err) => bail!(
                "Failed to render dependency graph for {pex}: {err}",
                pex = pex.path.display()
            ),
        };
        if args.open {
            let mut file = if let Some(path) = args.output {
                File::create(path)?
            } else {
                let temp = tempfile::Builder::new()
                    .prefix("pexrc-tools-graph.")
                    .suffix(&format!(".deps.{fmt}"))
                    .tempfile()?;
                let (file, path) = temp.keep()?;
                File::from_parts(file, path)
            };
            file.write_all(rendered.as_slice())?;
            open::that_detached(file.path())?;
        } else {
            let mut output = Output::new(args.output.as_deref())?;
            output.write_all(rendered.as_slice())?;
        }
    } else {
        let mut output = Output::new(args.output.as_deref())?;
        writeln!(&mut output, "{graph}", graph = graph.print(&mut ctx))?;
    }
    Ok(())
}
