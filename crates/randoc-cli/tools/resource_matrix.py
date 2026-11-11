#!/usr/bin/env python3
"""Records how Pandoc finds and reads resource files (include files and templates).

Usage: resource_matrix.py PANDOC OUT.json [resources|utf8|GROUP [OLD.json ...]]

`resources` records each group except `utf8`. GROUP is one of `base`,
`lists`, `templates`, `defaults`, `named`, `datadir`, `aux`, `values`. A case that has a record with the
same arguments and files in an OLD.json keeps that record; the oracle runs
only for the other cases.

Each case runs PANDOC once in a new folder with the case files. The data file
keeps the arguments, the files (in hexadecimal), the exit code, standard output, standard error
and the log bytes. The text `{root}` stands for the case folder.
`tests/resources.rs` replays each case against Randoc.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile

DOC = '{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[]}'

NAMES = ["inc.txt", "./inc.txt", "sub/inc.txt", "sub/../inc.txt", "sub//inc.txt",
         "{root}/inc.txt", "nosuch.txt"]
RESOURCE_PATHS = [None, "", ":r1", "r1:", "r1/", "./", ".", "./r1", "nosuchdir",
                  "r1:r2", "r2:r1", "./sub"]
# Where the include file exists: the working folder and the folders r1 and r2.
PLACES = [("cwd",), ("r1",), ("r2",), ("cwd", "r1"), ("r1", "r2"), ()]


def place_files(name, places):
    """The files for an include NAME that exists in each of PLACES."""
    files = {"sub/keep": "", "r1/sub/keep": "", "r2/sub/keep": ""}
    relative = "inc.txt" if name.startswith("{root}/") else name
    for place in places:
        prefix = "" if place == "cwd" else place + "/"
        files[os.path.normpath(prefix + relative)] = place + "\n"
    return files


def include_cases():
    cases = []
    for sandbox in [False, True]:
        for name in NAMES if not sandbox else ["inc.txt", "sub/inc.txt", "{root}/inc.txt"]:
            places = [()] if name == "nosuch.txt" else PLACES
            for rp in RESOURCE_PATHS if not sandbox else [None, ":r1", "r1/", "./r1", "r1:r2"]:
                for where in places:
                    args = ["-f", "json", "-t", "json", "--verbose", "--log=log.json"]
                    if sandbox:
                        args.insert(0, "--sandbox")
                    if rp is not None:
                        args.append("--resource-path=" + rp)
                    args += ["-H", name]
                    cases.append({"args": args, "files": place_files(name, where)})
    # The other include options, the defaults keys and the verbosity levels.
    for option, key in [("-H", "include-in-header"), ("-B", "include-before-body"),
                        ("-A", "include-after-body")]:
        for rp in [None, ":r1", "r1/", "./r1", "nosuchdir"]:
            for where in [("cwd",), ("r1",), ("cwd", "r1"), ()]:
                files = place_files("inc.txt", where)
                base = ["-f", "json", "-t", "json", "--log=log.json"]
                if rp is not None:
                    base.append("--resource-path=" + rp)
                for level in ["--verbose", "--quiet", None]:
                    args = base + ([level] if level else [])
                    cases.append({"args": args + [option, "inc.txt"], "files": files})
                for ext, text in [("yaml", key + ":\n- inc.txt\n"),
                                  ("json", json.dumps({key: ["inc.txt"]}))]:
                    with_defaults = dict(files)
                    with_defaults["d." + ext] = text
                    cases.append({"args": base + ["--verbose", "-d", "d." + ext],
                                  "files": with_defaults})
    return cases


def template_cases():
    cases = []
    names = ["tpl", "tpl.json", "./tpl", "sub/tpl", "{root}/tpl"]
    layouts = [
        {"tpl": "bare\n"},
        {"tpl.json": "format\n"},
        {"tpl": "bare\n", "tpl.json": "format\n"},
        {"dd/templates/tpl.json": "data\n"},
        {"dd/templates/tpl": "data bare\n"},
        {},
    ]
    for sandbox in [False, True]:
        for name in names:
            for layout in layouts:
                files = {"sub/keep": ""}
                for path, text in layout.items():
                    if name.startswith("sub/") and not path.startswith("dd/"):
                        path = "sub/" + path
                    files[path] = text
                for level in ["--verbose", "--quiet"]:
                    args = ["-f", "json", "-t", "json", level, "--log=log.json",
                            "--data-dir=dd", "--template=" + name]
                    if sandbox:
                        args.insert(0, "--sandbox")
                    cases.append({"args": args, "files": files})
    return cases


def order_cases():
    """The order of the reads: templates, include files, inputs and errors."""
    cases = []
    good = {"h.txt": "h\n", "b.txt": "b\n", "a.txt": "a\n", "tpl.json": "t\n",
            "in.json": DOC}
    all_kinds = ["-H", "h.txt", "-B", "b.txt", "-A", "a.txt", "--template=tpl"]
    for sandbox in [False, True]:
        lead = ["--sandbox"] if sandbox else []
        for level in ["--verbose", "--quiet"]:
            base = lead + ["-f", "json", "-t", "json", level, "--log=log.json"]
            cases.append({"args": base + all_kinds + ["in.json"], "files": good})
            cases.append({"args": base + ["-H", "h.txt", "-H", "a.txt", "-B", "b.txt",
                                          "-B", "h.txt", "-A", "a.txt", "in.json"],
                          "files": good})
            for bad_kind in ["-H", "-B", "-A"]:
                for other in ["-H", "-B", "-A"]:
                    if other == bad_kind:
                        continue
                    files = {"bad.txt": "\udcff", "in.json": DOC}
                    cases.append({"args": base + [bad_kind, "bad.txt", other, "missing.txt",
                                                  "in.json"], "files": files})
            for missing in ["--template=nosuch", "-H", "-B"]:
                args = base + ["-H", "h.txt", "-B", "b.txt", "-A", "a.txt"]
                args += [missing] if missing.startswith("--") else [missing, "missing.txt"]
                cases.append({"args": args + ["--template=tpl", "nosuch.json"],
                              "files": good})
    return cases


def sandbox_message_cases():
    cases = []
    latin = '{"pandoc-api-version":[1,23,1,2],"meta":{"a":{"t":"MetaString","c":"\udce9"}},"blocks":[]}'
    for sandbox in [False, True]:
        lead = ["--sandbox"] if sandbox else []
        for level in [None, "--verbose", "--quiet"]:
            extra = [level] if level else []
            cases.append({"args": lead + ["-f", "json", "-t", "json"] + extra + ["lat.json"],
                          "files": {"lat.json": latin}})
            cases.append({"args": lead + ["-f", "json", "-t", "json"] + extra +
                          ["--resource-path=r1", "-H", "only.txt"],
                          "files": {"r1/only.txt": "only\n"}})
    return cases


BASE = ["-f", "json", "-t", "json", "--verbose", "--log=log.json"]


def case(args, files, sandbox=False):
    """A case that reads the input file `a.json`."""
    files = dict(files)
    files["a.json"] = DOC
    return {"args": (["--sandbox"] if sandbox else []) + BASE + args + ["a.json"],
            "files": files}


def placed(name, places, folders=("r1", "r2", "r3")):
    """The include file NAME in each of PLACES; each folder of FOLDERS exists."""
    files = {folder + "/keep": "" for folder in folders}
    for place in places:
        prefix = "" if place == "cwd" else place + "/"
        files[os.path.normpath(prefix + name)] = place + "\n"
    return files


def list_cases():
    """The resource path as a list: a repeated option, the key of a defaults
    file together with the option, and the forms of a folder."""
    cases = []
    place_sets = [("cwd", "r1", "r2", "r3"), ("cwd", "r1", "r2"), ("cwd", "r1", "r3"),
                  ("cwd", "r2", "r3"), ("cwd", "r1"), ("cwd", "r2"), ("cwd",), ()]
    few_sets = [("cwd", "r1", "r2", "r3"), ("cwd", "r1"), ("cwd",), ("r1",)]
    repeated = [["r1", "r2"], ["r1", "r2", "r3"], ["r1:r2", "r3"], ["r3", "r1:r2"],
                [".", "r1"], ["r1", "."], ["", "r1"], ["r1", ""], ["r1", "r1"]]
    for values in repeated:
        options = ["--resource-path=" + value for value in values]
        for places in place_sets:
            cases.append(case(options + ["-H", "inc.txt"], placed("inc.txt", places)))
        for places in [("cwd", "r1", "r2", "r3"), ("cwd",)]:
            cases.append(case(options + ["-B", "./inc.txt"], placed("inc.txt", places)))
        cases.append(case(options + ["-H", "inc.txt"],
                          placed("inc.txt", ("cwd", "r1", "r2", "r3")), sandbox=True))

    # The key `resource-path` of a defaults file, alone and with the option.
    keys = [["r1"], ["r1", "r2"], ["", "r1"], ["."], [], [""]]
    for key in keys:
        for ext in ["yaml", "json"]:
            text = json.dumps({"resource-path": key})
            if ext == "yaml":
                text = "resource-path: " + json.dumps(key) + "\n"
            lines = [["-d", "d." + ext]]
            if ext == "json" and key not in (["r1"], [""]):
                continue
            if ext == "yaml":
                lines += [["-d", "d." + ext, "--resource-path=r3"],
                          ["--resource-path=r3", "-d", "d." + ext]]
            for line in lines:
                for places in few_sets:
                    for name in ["inc.txt", "./inc.txt"]:
                        if name == "./inc.txt" and places not in [("cwd",), ("r1",)]:
                            continue
                        files = placed("inc.txt", places)
                        files["d." + ext] = text
                        cases.append(case(line + ["-H", name], files))
            if ext == "yaml":
                files = placed("inc.txt", ("cwd", "r1", "r2", "r3"))
                files["d." + ext] = text
                cases.append(case(["-d", "d." + ext, "--resource-path=r3", "-H", "inc.txt"],
                                  files, sandbox=True))
    # Two defaults files that each set the key, and a key with the include key.
    two = {"d1.yaml": "resource-path: [r1]\n", "d2.yaml": "resource-path: [r2]\n",
           "d3.yaml": "resource-path: [r2]\ninclude-in-header: [inc.txt]\n"}
    for line in [["-d", "d1.yaml", "-d", "d2.yaml", "-H", "inc.txt"],
                 ["-d", "d2.yaml", "-d", "d1.yaml", "-H", "inc.txt"],
                 ["-d", "d1.yaml", "-d", "d2.yaml", "--resource-path=r3", "-H", "inc.txt"],
                 ["-d", "d1.yaml", "--resource-path=r3", "-d", "d2.yaml", "-H", "inc.txt"],
                 ["-d", "d1.yaml", "-d", "d3.yaml"], ["-d", "d3.yaml", "-d", "d1.yaml"],
                 ["--resource-path=r3", "-d", "d3.yaml"]]:
        for places in few_sets:
            files = placed("inc.txt", places)
            files.update(two)
            cases.append(case(line, files))

    # The forms of a folder. The include file is in `r1`, and in `r0/r1` for
    # the forms that name `r0`.
    forms = ["r1/", "r1//", "r1///", ".//r1", "./r1/", "./r1//", "r1/.", "r1/./", "./.",
             "././r1", ".//", "r0//r1", "r0/r1/", "r0/./r1", "r0/../r1", "r1/../r1",
             "r1//../r1", "{root}/r1", "{root}/r1/", "{root}//r1", "{root}/r1//"]
    for form in forms:
        folder = "r0/r1" if form.startswith("r0") and ".." not in form else "r1"
        for name in ["inc.txt", "./inc.txt", "sub/inc.txt"]:
            for places in [(folder,), ("cwd", folder)]:
                files = placed(name, places, folders=("r0", "r0/r1", "r1", "r1/sub", "sub"))
                cases.append(case(["--resource-path=" + form, "-A", name], files))
        if form in ["r1//", ".//r1", "./.", "r0//r1", "{root}/r1/"]:
            files = placed("inc.txt", (folder,), folders=("r0", "r0/r1", "r1"))
            cases.append(case(["--resource-path=" + form, "-H", "inc.txt"], files,
                              sandbox=True))
        if form in ["r1/", "r1//", ".//r1", "./r1/", "r1/.", "./.", ".//", "r0//r1"]:
            files = placed("inc.txt", ("cwd", folder), folders=("r0", "r0/r1", "r1"))
            files["d.yaml"] = "resource-path: " + json.dumps([form]) + "\n"
            for name in ["inc.txt", "./inc.txt"]:
                cases.append(case(["-d", "d.yaml", "-H", name], files))
    return cases


def template_path_cases():
    """Templates with a resource path: a template in the working folder, in a
    resource folder, in the `templates` folder of the data directory."""
    cases = []
    places = ["cwd", "r1", "dd"]
    subsets = [tuple(p for i, p in enumerate(places) if mask >> i & 1) for mask in range(8)]

    def layout(name, spec):
        """SPEC maps a place to the variants of the file: `bare`, `format`."""
        files = {"r1/keep": "", "sub/keep": "", "r1/sub/keep": "", "dd/templates/keep": ""}
        base = name.split("/")[-1]
        for place, variants in spec.items():
            for variant in variants:
                suffix = ".json" if variant == "format" else ""
                path = {"cwd": name, "r1": "r1/" + name, "dd": "dd/templates/" + base}[place]
                files[os.path.normpath(path + suffix)] = place + " " + variant + "\n"
        return files

    def layouts(name):
        specs = [{place: [variant] for place in subset}
                 for subset in subsets for variant in ["bare", "format"] if subset]
        specs.append({})
        specs += [{"cwd": ["bare"], "r1": ["format"]}, {"cwd": ["format"], "r1": ["bare"]},
                  {"cwd": ["bare", "format"], "r1": ["bare", "format"]},
                  {"r1": ["bare"], "dd": ["format"]}, {"r1": ["format"], "dd": ["bare"]},
                  {"cwd": ["bare"], "dd": ["format"]}, {"cwd": ["format"], "dd": ["bare"]}]
        return [layout(name, spec) for spec in specs]

    for name in ["tpl", "tpl.json", ".tpl", "sub/tpl", "tpl.x", "./tpl"]:
        for files in layouts(name):
            cases.append(case(["--data-dir=dd", "--resource-path=r1", "--template=" + name],
                              files))
    for name in ["tpl", "tpl.json"]:
        for files in layouts(name):
            cases.append(case(["--data-dir=dd", "--resource-path=.:r1", "--template=" + name],
                              files))
            cases.append(case(["--data-dir=dd", "--resource-path=r1", "--template=" + name],
                              files, sandbox=True))
    for files in layouts("tpl"):
        files["d.yaml"] = "template: tpl\nresource-path: [r1]\ndata-dir: dd\n"
        cases.append(case(["-d", "d.yaml"], files))
    for name in [".tpl", "tpl.x", "sub/tpl", "tpl.", "sub.d/tpl"]:
        for files in [layout(name, {"cwd": ["bare"]}), layout(name, {"cwd": ["format"]}), {}]:
            cases.append(case(["--template=" + name], files))

    # A name with a folder part and the `templates` folder of the data directory.
    for layout_files in [{"dd/templates/sub/tpl": "data sub bare\n"},
                         {"dd/templates/sub/tpl.json": "data sub format\n"},
                         {"dd/templates/sub/tpl": "data sub bare\n",
                          "dd/templates/tpl": "data bare\n"},
                         {"dd/templates/sub/tpl.json": "data sub format\n",
                          "dd/templates/tpl.json": "data format\n"},
                         {"dd/templates/sub/tpl.json": "data sub format\n",
                          "sub/tpl.json": "cwd format\n"}]:
        for name in ["sub/tpl", "sub/tpl.json", "./tpl", "{root}/sub/tpl"]:
            cases.append(case(["--data-dir=dd", "--template=" + name], layout_files))

    # A template that is named directly.
    direct = {"dd/templates/tpl.json": "data\n", "r1/tpl.json": "resource\n",
              "r1/r1/keep": ""}
    for name in ["dd/templates/tpl.json", "dd/templates/tpl", "r1/tpl.json",
                 "{root}/dd/templates/tpl.json", "{root}/r1/tpl.json", "./r1/tpl.json"]:
        for level in ["--verbose", "--quiet"]:
            args = ["-f", "json", "-t", "json", level, "--log=log.json", "--data-dir=dd",
                    "--resource-path=r1", "--template=" + name, "a.json"]
            cases.append({"args": args, "files": dict(direct, **{"a.json": DOC})})

    # A template file that is not UTF-8.
    bad = ["\udcff", "﻿\udcff", "ab\udce2\udc82", "ab\udcc2\x00z"]
    for index, text in enumerate(bad):
        for place in ["bad.json", "r1/bad.json", "dd/templates/bad.json"]:
            for name in ["bad.json", "bad"]:
                if index and (place != "bad.json" or name != "bad.json"):
                    continue
                cases.append(case(["--data-dir=dd", "--resource-path=r1",
                                   "--template=" + name], {place: text, "r1/keep": ""}))
    cases.append(case(["--template=bad.json"], {"bad.json": "\udcff"}, sandbox=True))
    for place, name in [("sub/bad.json", "sub/bad.json"), ("sub/bad.json", "sub/bad"),
                        ("dd/templates/bad.json", "sub/bad.json"),
                        ("r1/sub/bad.json", "sub/bad.json")]:
        cases.append(case(["--data-dir=dd", "--resource-path=r1", "--template=" + name],
                          {place: "\udcff"}))
    return cases


def defaults_search_cases():
    """The search for a defaults file: each candidate is absent, a file or a folder."""
    cases = []

    def tree(states):
        files = {"dd/defaults/keep": ""}
        for path, state in states.items():
            if state == "file" and path == "a":
                files[path] = ""
            elif state == "file":
                files[path] = "metadata:\n  src: " + path + "\n"
            elif state == "folder":
                files[path + "/keep"] = ""
        return files

    states = ["absent", "file", "folder"]
    for a in states:
        for b in states:
            for c in states:
                for d in states:
                    files = tree({"x": a, "x.yaml": b, "dd/defaults/x": c,
                                  "dd/defaults/x.yaml": d})
                    cases.append(case(["--data-dir=dd", "-d", "x"], files))
    for name, local in [("x.yaml", "x.yaml"), ("x.json", "x.json"), ("sub/x", "sub/x.yaml"),
                        ("dd", "dd.yaml")]:
        for a in states:
            for b in states:
                in_data = "dd/defaults/" + (local if name != "dd" else "dd.yaml")
                files = tree({local: a, in_data: b})
                cases.append(case(["--data-dir=dd", "-d", name], files))
    # A name with an extension: Pandoc also tries NAME.yaml.
    for name in ["q.foo", "q.yml", "q.JSON", "q.yaml", "q.", "q.config", "q.json"]:
        for place in [name + ".yaml", "dd/defaults/" + name + ".yaml", name,
                      "dd/defaults/" + name]:
            cases.append(case(["--data-dir=dd", "-d", name], tree({place: "file"})))
    # A name with a folder part: the local folder is a file, or it is missing.
    for name in ["a/x", "a/x.yaml", "sub/x", "a/b/x"]:
        for local in ["file", "absent"]:
            for data in ["dd/defaults/" + name, "dd/defaults/" + name + ".yaml", None]:
                states = {"a": local}
                if data:
                    states[data] = "file"
                cases.append(case(["--data-dir=dd", "-d", name], tree(states)))
    return cases


NAMED = [("--bibliography", "n.bib"), ("--csl", "n.csl"), ("--reference-doc", "n.docx"),
         ("--epub-embed-font", "n.ttf"), ("--syntax-definition", "n.xml"),
         ("--abbreviations", "n.abbr"), ("--citation-abbreviations", "n.cabbr"),
         ("--lua-filter", "n.lua"), ("--filter", "n.filt"), ("--metadata-file", "n.yaml"),
         ("--template", "n.tpl"), ("--highlight-style", "n.theme"),
         ("--syntax-highlighting", "s.theme"), ("--css", "n.css"),
         ("--include-in-header", "n.h"), ("--include-before-body", "n.b"),
         ("--include-after-body", "n.a"), ("--epub-cover-image", "n.png"),
         ("--epub-metadata", "n.meta"), ("INPUT", "n.json")]


def named_arg(option, name):
    """The argument for a named file; `INPUT` stands for an input file."""
    return name if option == "INPUT" else option + "=" + name


def named_file_cases(pandoc, home):
    """Each option that names a file, when the file is missing: alone, with
    the file in a resource folder only, two files for one option, and in
    pairs. The pairs take the options whose missing file stops Pandoc, so they
    show the order of the reads. The last cases name a folder as the output
    file and as the log file."""
    cases = []
    for sandbox in [False, True]:
        singles = [case([named_arg(option, name)], {}, sandbox) for option, name in NAMED]
        cases += singles
        for option, name in NAMED:
            cases.append(case(["--resource-path=sub", named_arg(option, name)],
                              {"sub/" + name: ""}, sandbox))
            cases.append(case([named_arg(option, "1" + name), named_arg(option, "2" + name)],
                              {}, sandbox))
        stops = [index for index, single in enumerate(singles)
                 if run(pandoc, single, home)["exit"] != 0]
        for i in stops:
            for j in stops:
                if i < j:
                    first, second = (i, j) if (i + j) % 2 == 0 else (j, i)
                    args = [named_arg(*NAMED[k]) for k in (first, second)]
                    cases.append(case(args, {}, sandbox))
        # A metadata file that is not valid, with an input file that is
        # missing; a missing named file, with an input file that is not valid.
        cases.append(case(["--metadata-file=m.yaml", "n.json"], {"m.yaml": "- a\n"}, sandbox))
        for option, name in NAMED[:4] + NAMED[7:9]:
            cases.append(case([named_arg(option, name), "broken.json"], {"broken.json": "{"},
                              sandbox))
        cases.append(case(["-o", "dir"], {"dir/keep": ""}, sandbox))
        cases.append(case(["--log=dir"], {"dir/keep": ""}, sandbox))
        # A named file that is a folder. `--lua-filter` is row I-CLI-26.
        for option, name in NAMED:
            if option != "--lua-filter":
                cases.append(case([named_arg(option, name)], {name + "/keep": ""}, sandbox))
    return cases


DATA_KINDS = [
    # (kind, arguments, path in the data folder, valid text)
    ("metadata", ["--metadata-file=k.yaml"], "metadata/k.yaml", "k: data\n"),
    ("defaults", ["-d", "k"], "defaults/k.yaml", "metadata:\n  k: data\n"),
    ("template", ["--template=k"], "templates/k.json", "data\n"),
    ("abbreviations", [], "abbreviations", "Dr.\n"),
    ("csl", ["--csl=k.csl"], "csl/k.csl", "<style/>\n"),
    ("reference-doc", ["--reference-doc=k.docx"], "k.docx", "doc\n"),
    ("syntax-definition", ["--syntax-definition=k.xml"], "k.xml", "<language/>\n"),
    ("citation-abbreviations", ["--citation-abbreviations=k.json"], "k.json", "{}\n"),
    ("highlight-style", ["--highlight-style=k.theme"], "k.theme", "{}\n"),
    ("epub-metadata", ["--epub-metadata=k.xml"], "k.xml", "<dc/>\n"),
    ("bibliography", ["--bibliography=k.bib"], "k.bib", "\n"),
]


def data_folder_cases():
    """Each kind of file that Pandoc can look for in the user data folder:
    the file present, missing, a folder and not UTF-8, with and without
    `--sandbox`, with `--data-dir` and with `XDG_DATA_HOME`."""
    cases = []
    for kind, args, path, valid in DATA_KINDS:
        if kind in ("syntax-definition",):
            states = [("missing", None), ("folder", None)]
        else:
            states = [("present", valid), ("missing", None), ("folder", None),
                      ("not-utf8", "\udcff\n")]
        for state, text in states:
            for where in ["data-dir", "xdg"]:
                prefix = "dd/" if where == "data-dir" else "xdg/pandoc/"
                files = {"dd/keep": "", "xdg/pandoc/keep": ""}
                if state == "folder":
                    files[prefix + path + "/keep"] = ""
                elif text is not None:
                    files[prefix + path] = text
                for sandbox in [False, True]:
                    extra = ["--data-dir=dd"] if where == "data-dir" else []
                    c = case(extra + args, files, sandbox)
                    c["env"] = {"XDG_DATA_HOME": "{root}/xdg"}
                    cases.append(c)
    return cases


AUX_KINDS = [
    # (kind, arguments, file, valid text)
    ("abbreviations", ["--abbreviations=f"], "f", "Dr.\nProf.\n"),
    ("epub-metadata", ["--epub-metadata=f"], "f", "<dc:title>t</dc:title>\n"),
    ("metadata-file", ["--metadata-file=f"], "f", "a: b\nc: d\n"),
    ("include", ["-H", "f"], "f", "a\nb\n"),
    ("template", ["--template=f"], "f", "a\nb\n"),
    ("highlight-style", ["--highlight-style=f.theme"], "f.theme", "{}\n"),
    ("citation-abbreviations", ["--citation-abbreviations=f"], "f", "{}\n"),
    ("css", ["--css=f"], "f", "a {}\n"),
    ("bibliography", ["--bibliography=f.bib"], "f.bib", "\n"),
]


def utf8_rule_cases():
    """Each file that the program reads as text: valid UTF-8, not UTF-8, a
    byte order mark and CR line ends."""
    cases = []
    for kind, args, name, valid in AUX_KINDS:
        variants = [valid, "\udcff", valid[:2] + "\udcff" + valid[2:], "﻿" + valid,
                    valid.replace("\n", "\r\n")]
        for text in variants:
            for sandbox in [False, True]:
                cases.append(case(args, {name: text}, sandbox))
    return cases


VALUE_KEYS = [
    # (key, option, names, files): the files exist when FILES is true.
    ("citation-abbreviations", "--citation-abbreviations", ["a", "b"], False),
    ("bibliography", "--bibliography", ["a.bib", "b.bib"], True),
    ("csl", "--csl", ["a.csl", "b.csl"], False),
    ("metadata-files", "--metadata-file", ["a.yaml", "b.yaml"], True),
    ("epub-fonts", "--epub-embed-font", ["a.ttf", "b.ttf"], False),
    ("syntax-definitions", "--syntax-definition", ["a.xml", "b.xml"], False),
]


def value_form_cases():
    """The value forms of each defaults key that takes a file name or a
    list, against the option, alone and together, in both orders."""
    cases = []
    for key, option, (a, b), exist in VALUE_KEYS:
        files = {}
        if exist:
            for name in [a, b, "c" + a[1:]]:
                files[name] = ("x: " + name + "\n") if name.endswith(".yaml") else "\n"
        values = {"scalar": a, "one": [a], "two": [a, b], "empty": None}
        for form, value in values.items():
            text = key + ":" + ("" if value is None else " " + json.dumps(value)) + "\n"
            with_d = dict(files, **{"d.yaml": text})
            other = option + "=c" + a[1:]
            for line in [["-d", "d.yaml"], ["-d", "d.yaml", other], [other, "-d", "d.yaml"],
                         [other, option + "=d" + a[1:], "-d", "d.yaml"]]:
                for sandbox in [False, True]:
                    cases.append(case(line, with_d, sandbox))
        for sandbox in [False, True]:
            cases.append(case([option + "=" + a, option + "=" + b], files, sandbox))
    return cases


def utf8_cases():
    """UTF-8 errors in an include file: lead bytes with second and third bytes."""
    leads = [0x80, 0xC0, 0xC1, 0xC2, 0xDF, 0xE0, 0xE1, 0xEC, 0xED, 0xEE, 0xEF,
             0xF0, 0xF1, 0xF3, 0xF4, 0xF5, 0xFF]
    seconds = [0x00, 0x41, 0x7F, 0x80, 0x8F, 0x90, 0x9F, 0xA0, 0xBF, 0xC0, 0xFF]
    prefixes = [bytes([lead, second]) for lead in leads for second in seconds]
    prefixes += [bytes([lead]) for lead in leads]
    for lead, second in [(0xE0, 0xA0), (0xED, 0x80), (0xED, 0x9F), (0xE1, 0x80),
                         (0xF0, 0x90), (0xF4, 0x8F), (0xF4, 0x80), (0xF1, 0x80)]:
        for third in [0x41, 0x80, 0xBF, 0xC0]:
            prefixes.append(bytes([lead, second, third]))
    cases = []
    for prefix in prefixes:
        text = decode(b"ab" + prefix + b"z")
        cases.append({"args": ["-f", "json", "-t", "json", "-H", "bad.txt"],
                      "files": {"bad.txt": text}})
    return cases


def encode(text):
    """Turns text with surrogate escapes into bytes."""
    return text.encode("utf-8", "surrogateescape")


def decode(data):
    return data.decode("utf-8", "surrogateescape")


# The records of the OLD.json files, by arguments and files.
STORED = {}


def identity(args, files, env=None):
    if env:
        return json.dumps([args, files, env], sort_keys=True)
    return json.dumps([args, files], sort_keys=True)


def run(pandoc, case, home):
    hex_files = {path: encode(text).hex() for path, text in case["files"].items()}
    stored = STORED.get(identity(case["args"], hex_files, case.get("env")))
    if stored is not None:
        return stored
    root = tempfile.mkdtemp(prefix="cli-resource-")
    try:
        for path, text in case["files"].items():
            full = os.path.join(root, path)
            os.makedirs(os.path.dirname(full), exist_ok=True)
            with open(full, "wb") as handle:
                handle.write(encode(text))
        args = [a.replace("{root}", root) for a in case["args"]]
        env = {"PATH": os.environ.get("PATH", ""), "HOME": home,
               "XDG_DATA_HOME": os.path.join(home, "share"), "GHCRTS": "-M512m",
               "LANG": "C.UTF-8"}
        for key, value in case.get("env", {}).items():
            env[key] = value.replace("{root}", root)
        uses_stdin = not any(a.endswith(".json") and not a.startswith("-") for a in args)
        result = subprocess.run([pandoc] + args, cwd=root, env=env, capture_output=True,
                                input=encode(DOC) if uses_stdin else b"", timeout=60)
        log_path = os.path.join(root, "log.json")
        log = None
        if os.path.isfile(log_path):
            with open(log_path, "rb") as handle:
                log = decode(handle.read()).replace(root, "{root}")
        files = {path: encode(text).hex() for path, text in case["files"].items()}
        return dict(case, files=files, exit=result.returncode,
                    stdout=decode(result.stdout).replace(root, "{root}"),
                    stderr=decode(result.stderr).replace(root, "{root}"), log=log)
    finally:
        shutil.rmtree(root)


def main():
    pandoc, out = sys.argv[1], sys.argv[2]
    mode = sys.argv[3] if len(sys.argv) > 3 else "resources"
    for old in sys.argv[4:]:
        with open(old, encoding="utf-8") as handle:
            for record in json.load(handle):
                STORED[identity(record["args"], record["files"], record.get("env"))] = record
    home = tempfile.mkdtemp(prefix="cli-resource-home-")
    groups = {
        "base": lambda: (include_cases() + template_cases() + order_cases()
                         + sandbox_message_cases()),
        "lists": list_cases,
        "templates": template_path_cases,
        "defaults": defaults_search_cases,
        "named": lambda: named_file_cases(pandoc, home),
        "utf8": utf8_cases,
        "datadir": data_folder_cases,
        "aux": utf8_rule_cases,
        "values": value_form_cases,
    }
    try:
        if mode == "resources":
            cases = [c for name in ["base", "lists", "templates", "defaults", "named",
                                    "datadir", "aux", "values"]
                     for c in groups[name]()]
        else:
            cases = groups[mode]()
        from concurrent.futures import ThreadPoolExecutor
        records = []
        # Two oracle processes at a time, the limit of the machine rules.
        with ThreadPoolExecutor(max_workers=2) as pool:
            for index, record in enumerate(pool.map(lambda c: run(pandoc, c, home), cases)):
                records.append(record)
                if index % 50 == 0:
                    print(f"{index}/{len(cases)}", file=sys.stderr, flush=True)
    finally:
        shutil.rmtree(home)
    with open(out, "w", encoding="utf-8") as handle:
        handle.write("[\n")
        handle.write(",\n".join(json.dumps(r, ensure_ascii=True, sort_keys=True)
                                for r in records))
        handle.write("\n]\n")
    print(f"{len(records)} cases")


if __name__ == "__main__":
    main()
