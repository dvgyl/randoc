// Generated from Pandoc 3.13 output for the inputs below. Do not edit by hand.

pub(super) const READER: &[(&str, &str, &str)] = &[
    (
        "results_keyword_in_affiliated_keywords",
        "#+begin_src r :exports results\nplot(1)\n#+end_src\n\n#+caption: Totals.\n#+name: totals\n#+RESULTS:\n| n | v |\n|---+---|\n| 1 | 2 |\n",
        "[ Table\n    ( \"totals\" , [] , [] )\n    (Caption Nothing [ Plain [ Str \"Totals.\" ] ])\n    [ ( AlignDefault , ColWidthDefault )\n    , ( AlignDefault , ColWidthDefault )\n    ]\n    (TableHead\n       ( \"\" , [] , [] )\n       [ Row\n           ( \"\" , [] , [] )\n           [ Cell\n               ( \"\" , [] , [] )\n               AlignDefault\n               (RowSpan 1)\n               (ColSpan 1)\n               [ Plain [ Str \"n\" ] ]\n           , Cell\n               ( \"\" , [] , [] )\n               AlignDefault\n               (RowSpan 1)\n               (ColSpan 1)\n               [ Plain [ Str \"v\" ] ]\n           ]\n       ])\n    [ TableBody\n        ( \"\" , [] , [] )\n        (RowHeadColumns 0)\n        []\n        [ Row\n            ( \"\" , [] , [] )\n            [ Cell\n                ( \"\" , [] , [] )\n                AlignDefault\n                (RowSpan 1)\n                (ColSpan 1)\n                [ Plain [ Str \"1\" ] ]\n            , Cell\n                ( \"\" , [] , [] )\n                AlignDefault\n                (RowSpan 1)\n                (ColSpan 1)\n                [ Plain [ Str \"2\" ] ]\n            ]\n        ]\n    ]\n    (TableFoot ( \"\" , [] , [] ) [])\n]\n",
    ),
    (
        "hidden_results_take_their_keywords",
        "#+begin_src r :exports none\nplot(1)\n#+end_src\n\n#+caption: Totals.\n#+RESULTS:\n| n |\n\nAfter.\n",
        "[ Para [ Str \"After.\" ] ]\n",
    ),
    (
        "results_keyword_before_fixed_width",
        "#+begin_src r :exports none\nx\n#+end_src\n#+caption: c\n#+RESULTS:\n: a\n",
        "[ RawBlock (Format \"org\") \"#+RESULTS:\"\n, CodeBlock ( \"\" , [] , [] ) \"a\\n\"\n]\n",
    ),
    (
        "results_keyword_without_source_block",
        "#+RESULTS:\n#+caption: Kept.\n| a |\n",
        "[ Table\n    ( \"\" , [] , [] )\n    (Caption Nothing [ Plain [ Str \"Kept.\" ] ])\n    [ ( AlignDefault , ColWidthDefault ) ]\n    (TableHead ( \"\" , [] , [] ) [])\n    [ TableBody\n        ( \"\" , [] , [] )\n        (RowHeadColumns 0)\n        []\n        [ Row\n            ( \"\" , [] , [] )\n            [ Cell\n                ( \"\" , [] , [] )\n                AlignDefault\n                (RowSpan 1)\n                (ColSpan 1)\n                [ Plain [ Str \"a\" ] ]\n            ]\n        ]\n    ]\n    (TableFoot ( \"\" , [] , [] ) [])\n]\n",
    ),
    (
        "org_block_parameters",
        "#+attr_html: :height 3em\n#+begin_box :size big :tone \"dark red\"\ntext\n#+end_box\n",
        "[ Div\n    ( \"\"\n    , [ \"box\" ]\n    , [ ( \"height\" , \"3em\" )\n      , ( \"size\" , \"big\" )\n      , ( \"tone\" , \"dark red\" )\n      ]\n    )\n    [ Para [ Str \"text\" ] ]\n]\n",
    ),
    (
        "python_block_parameters",
        "#+begin_box size=big tone=dark-red\ntext\n#+end_box\n",
        "[ Div\n    ( \"\"\n    , [ \"box\" ]\n    , [ ( \"size\" , \"big\" ) , ( \"tone\" , \"dark-red\" ) ]\n    )\n    [ Para [ Str \"text\" ] ]\n]\n",
    ),
    (
        "other_block_parameters",
        "#+begin_box size: big\ntext\n#+end_box\n\n#+begin_box :size\ntext\n#+end_box\n",
        "[ Div\n    ( \"\" , [ \"box\" ] , [ ( \"parameters\" , \"size: big\" ) ] )\n    [ Para [ Str \"text\" ] ]\n, Div\n    ( \"\" , [ \"box\" ] , [ ( \"parameters\" , \":size\" ) ] )\n    [ Para [ Str \"text\" ] ]\n]\n",
    ),
    (
        "dynamic_block",
        "#+BEGIN: summary :span week\n#+caption: Week.\n| a |\n#+END:\n\n#+begin: plain\ntext\n#+end:\n",
        "[ Div\n    ( \"\" , [ \"summary\" ] , [ ( \"span\" , \"week\" ) ] )\n    [ Table\n        ( \"\" , [] , [] )\n        (Caption Nothing [ Plain [ Str \"Week.\" ] ])\n        [ ( AlignDefault , ColWidthDefault ) ]\n        (TableHead ( \"\" , [] , [] ) [])\n        [ TableBody\n            ( \"\" , [] , [] )\n            (RowHeadColumns 0)\n            []\n            [ Row\n                ( \"\" , [] , [] )\n                [ Cell\n                    ( \"\" , [] , [] )\n                    AlignDefault\n                    (RowSpan 1)\n                    (ColSpan 1)\n                    [ Plain [ Str \"a\" ] ]\n                ]\n            ]\n        ]\n        (TableFoot ( \"\" , [] , [] ) [])\n    ]\n, Div ( \"\" , [ \"plain\" ] , [] ) [ Para [ Str \"text\" ] ]\n]\n",
    ),
    (
        "grid_table_with_head_and_blocks",
        "+------+-------+\n| Name | Notes |\n+:=====+======:+\n| ant  | small |\n|      | quick |\n+------+-------+\n| bee  | - one |\n|      | - two |\n+------+-------+\n",
        "[ Table\n    ( \"\" , [] , [] )\n    (Caption Nothing [])\n    [ ( AlignLeft , ColWidth 9.722222222222222e-2 )\n    , ( AlignRight , ColWidth 0.1111111111111111 )\n    ]\n    (TableHead\n       ( \"\" , [] , [] )\n       [ Row\n           ( \"\" , [] , [] )\n           [ Cell\n               ( \"\" , [] , [] )\n               AlignDefault\n               (RowSpan 1)\n               (ColSpan 1)\n               [ Para [ Str \"Name\" ] ]\n           , Cell\n               ( \"\" , [] , [] )\n               AlignDefault\n               (RowSpan 1)\n               (ColSpan 1)\n               [ Para [ Str \"Notes\" ] ]\n           ]\n       ])\n    [ TableBody\n        ( \"\" , [] , [] )\n        (RowHeadColumns 0)\n        []\n        [ Row\n            ( \"\" , [] , [] )\n            [ Cell\n                ( \"\" , [] , [] )\n                AlignDefault\n                (RowSpan 1)\n                (ColSpan 1)\n                [ Para [ Str \"ant\" ] ]\n            , Cell\n                ( \"\" , [] , [] )\n                AlignDefault\n                (RowSpan 1)\n                (ColSpan 1)\n                [ Para [ Str \"small\" , SoftBreak , Str \"quick\" ] ]\n            ]\n        , Row\n            ( \"\" , [] , [] )\n            [ Cell\n                ( \"\" , [] , [] )\n                AlignDefault\n                (RowSpan 1)\n                (ColSpan 1)\n                [ Para [ Str \"bee\" ] ]\n            , Cell\n                ( \"\" , [] , [] )\n                AlignDefault\n                (RowSpan 1)\n                (ColSpan 1)\n                [ BulletList\n                    [ [ Plain [ Str \"one\" ] ]\n                    , [ Plain [ Str \"two\" ] ]\n                    ]\n                ]\n            ]\n        ]\n    ]\n    (TableFoot ( \"\" , [] , [] ) [])\n]\n",
    ),
    (
        "grid_table_with_plain_cells",
        "#+caption: Tools\n#+name: tbl:tools\n+-----+-----+\n| saw | cut |\n| axe | hew |\n+=====+=====+\n| awl | bore|\n+-----+-----+\n",
        "[ Table\n    ( \"tbl:tools\" , [] , [] )\n    (Caption Nothing [ Plain [ Str \"Tools\" ] ])\n    [ ( AlignDefault , ColWidth 8.333333333333333e-2 )\n    , ( AlignDefault , ColWidth 8.333333333333333e-2 )\n    ]\n    (TableHead\n       ( \"\" , [] , [] )\n       [ Row\n           ( \"\" , [] , [] )\n           [ Cell\n               ( \"\" , [] , [] )\n               AlignDefault\n               (RowSpan 1)\n               (ColSpan 1)\n               [ Plain [ Str \"saw\" , SoftBreak , Str \"axe\" ] ]\n           , Cell\n               ( \"\" , [] , [] )\n               AlignDefault\n               (RowSpan 1)\n               (ColSpan 1)\n               [ Plain [ Str \"cut\" , SoftBreak , Str \"hew\" ] ]\n           ]\n       ])\n    [ TableBody\n        ( \"\" , [] , [] )\n        (RowHeadColumns 0)\n        []\n        [ Row\n            ( \"\" , [] , [] )\n            [ Cell\n                ( \"\" , [] , [] )\n                AlignDefault\n                (RowSpan 1)\n                (ColSpan 1)\n                [ Plain [ Str \"awl\" ] ]\n            , Cell\n                ( \"\" , [] , [] )\n                AlignDefault\n                (RowSpan 1)\n                (ColSpan 1)\n                [ Plain [ Str \"bore\" ] ]\n            ]\n        ]\n    ]\n    (TableFoot ( \"\" , [] , [] ) [])\n]\n",
    ),
    (
        "grid_table_column_span",
        "+---+---+\n| wide  |\n+---+---+\n| a | b |\n+---+---+\n",
        "[ Table\n    ( \"\" , [] , [] )\n    (Caption Nothing [])\n    [ ( AlignDefault , ColWidth 5.555555555555555e-2 )\n    , ( AlignDefault , ColWidth 5.555555555555555e-2 )\n    ]\n    (TableHead ( \"\" , [] , [] ) [])\n    [ TableBody\n        ( \"\" , [] , [] )\n        (RowHeadColumns 0)\n        []\n        [ Row\n            ( \"\" , [] , [] )\n            [ Cell\n                ( \"\" , [] , [] )\n                AlignDefault\n                (RowSpan 1)\n                (ColSpan 2)\n                [ Plain [ Str \"wide\" ] ]\n            ]\n        , Row\n            ( \"\" , [] , [] )\n            [ Cell\n                ( \"\" , [] , [] )\n                AlignDefault\n                (RowSpan 1)\n                (ColSpan 1)\n                [ Plain [ Str \"a\" ] ]\n            , Cell\n                ( \"\" , [] , [] )\n                AlignDefault\n                (RowSpan 1)\n                (ColSpan 1)\n                [ Plain [ Str \"b\" ] ]\n            ]\n        ]\n    ]\n    (TableFoot ( \"\" , [] , [] ) [])\n]\n",
    ),
    (
        "wide_grid_table",
        "+------------------------------------------------------------+------------------------------+\n| x                                                          | y                            |\n+------------------------------------------------------------+------------------------------+\n",
        "[ Table\n    ( \"\" , [] , [] )\n    (Caption Nothing [])\n    [ ( AlignDefault , ColWidth 0.6630434782608695 )\n    , ( AlignDefault , ColWidth 0.33695652173913043 )\n    ]\n    (TableHead ( \"\" , [] , [] ) [])\n    [ TableBody\n        ( \"\" , [] , [] )\n        (RowHeadColumns 0)\n        []\n        [ Row\n            ( \"\" , [] , [] )\n            [ Cell\n                ( \"\" , [] , [] )\n                AlignDefault\n                (RowSpan 1)\n                (ColSpan 1)\n                [ Plain [ Str \"x\" ] ]\n            , Cell\n                ( \"\" , [] , [] )\n                AlignDefault\n                (RowSpan 1)\n                (ColSpan 1)\n                [ Plain [ Str \"y\" ] ]\n            ]\n        ]\n    ]\n    (TableFoot ( \"\" , [] , [] ) [])\n]\n",
    ),
    (
        "grid_table_without_end",
        "+-----+\n| a   |\n",
        "[ Para [ Strikeout [ Str \"\\8212\\8211\" ] ]\n, Table\n    ( \"\" , [] , [] )\n    (Caption Nothing [])\n    [ ( AlignDefault , ColWidthDefault ) ]\n    (TableHead ( \"\" , [] , [] ) [])\n    [ TableBody\n        ( \"\" , [] , [] )\n        (RowHeadColumns 0)\n        []\n        [ Row\n            ( \"\" , [] , [] )\n            [ Cell\n                ( \"\" , [] , [] )\n                AlignDefault\n                (RowSpan 1)\n                (ColSpan 1)\n                [ Plain [ Str \"a\" ] ]\n            ]\n        ]\n    ]\n    (TableFoot ( \"\" , [] , [] ) [])\n]\n",
    ),
    (
        "single_bar_table",
        "|\n| a |\n",
        "[ Table\n    ( \"\" , [] , [] )\n    (Caption Nothing [])\n    []\n    (TableHead ( \"\" , [] , [] ) [])\n    [ TableBody\n        ( \"\" , [] , [] )\n        (RowHeadColumns 0)\n        []\n        [ Row ( \"\" , [] , [] ) [] , Row ( \"\" , [] , [] ) [] ]\n    ]\n    (TableFoot ( \"\" , [] , [] ) [])\n]\n",
    ),
    (
        "list_marker_line_is_text",
        "- 3. items\n- | cell |\n- : fixed\n- -----\n- #+caption: c\n",
        "[ BulletList\n    [ [ Plain [ Str \"3.\" , Space , Str \"items\" ] ]\n    , [ Plain [ Str \"|\" , Space , Str \"cell\" , Space , Str \"|\" ]\n      ]\n    , [ Plain [ Str \":\" , Space , Str \"fixed\" ] ]\n    , [ Plain [ Str \"\\8212\\8211\" ] ]\n    , [ Plain [ Str \"#+caption:\" , Space , Str \"c\" ] ]\n    ]\n]\n",
    ),
    (
        "list_block_keeps_indentation",
        "- item\n  #+begin_example -i\n      deep\n  #+end_example\n     - inner\n          #+begin_example -i\n   shallow\n          #+end_example\n       inner text\n  outer text\nDone.\n",
        "[ BulletList\n    [ [ Para [ Str \"item\" ]\n      , CodeBlock ( \"\" , [] , [] ) \"      deep\\n\"\n      , BulletList\n          [ [ Para [ Str \"inner\" ]\n            , CodeBlock ( \"\" , [] , [] ) \"   shallow\\n\"\n            , Para [ Str \"inner\" , Space , Str \"text\" ]\n            ]\n          ]\n      , Para [ Str \"outer\" , Space , Str \"text\" ]\n      ]\n    ]\n, Para [ Str \"Done.\" ]\n]\n",
    ),
    (
        "exclude_tags_accumulate",
        "#+EXCLUDE_TAGS: red\n#+EXCLUDE_TAGS: blue\n* One :red:\n* Two :blue:\n* Three :noexport:\n",
        "[ Header\n    1\n    ( \"three\" , [] , [] )\n    [ Str \"Three\"\n    , Space\n    , Span\n        ( \"\" , [ \"tag\" ] , [ ( \"tag-name\" , \"noexport\" ) ] )\n        [ SmallCaps [ Str \"noexport\" ] ]\n    ]\n]\n",
    ),
    (
        "inline_source_blocks",
        "Run src_sh{ls -l} and src_python[:exports none]{x()} now, src_r[:var n=1]{n} (src_c++{x}).\n",
        "[ Para\n    [ Str \"Run\"\n    , Space\n    , Code\n        ( \"\" , [ \"bash\" ] , [ ( \"org-language\" , \"sh\" ) ] ) \"ls -l\"\n    , Space\n    , Str \"and\"\n    , Space\n    , Str \"now,\"\n    , Space\n    , Code ( \"\" , [ \"r\" ] , [ ( \"var\" , \"n=1\" ) ] ) \"n\"\n    , Space\n    , Str \"(src\"\n    , Subscript [ Str \"c\" ]\n    , Str \"++{x}).\"\n    ]\n]\n",
    ),
    (
        "windows_file_links",
        "[[file:c:/tmp/a.txt][A]] [[e:/b.txt][B]] [[file:/abs.txt][C]] [[file:x:y][D]]\n",
        "[ Para\n    [ Link\n        ( \"\" , [] , [] ) [ Str \"A\" ] ( \"file:///c:/tmp/a.txt\" , \"\" )\n    , Space\n    , Link\n        ( \"\" , [] , [] ) [ Str \"B\" ] ( \"file:///e:/b.txt\" , \"\" )\n    , Space\n    , Link\n        ( \"\" , [] , [] ) [ Str \"C\" ] ( \"file:///abs.txt\" , \"\" )\n    , Space\n    , Link ( \"\" , [] , [] ) [ Str \"D\" ] ( \"x:y\" , \"\" )\n    ]\n]\n",
    ),
    (
        "org_ref_citations",
        "See cite:&smith2020,jones-2019. and citep:lee, citeyear:kim_ (citet*:&ng).\n",
        "[ Para\n    [ Str \"See\"\n    , Space\n    , Cite\n        [ Citation\n            { citationId = \"smith2020\"\n            , citationPrefix = []\n            , citationSuffix = []\n            , citationMode = AuthorInText\n            , citationNoteNum = 0\n            , citationHash = 0\n            }\n        , Citation\n            { citationId = \"jones-2019\"\n            , citationPrefix = []\n            , citationSuffix = []\n            , citationMode = AuthorInText\n            , citationNoteNum = 0\n            , citationHash = 0\n            }\n        ]\n        [ Str \"cite:&smith2020,jones-2019\" ]\n    , Str \".\"\n    , Space\n    , Str \"and\"\n    , Space\n    , Cite\n        [ Citation\n            { citationId = \"lee\"\n            , citationPrefix = []\n            , citationSuffix = []\n            , citationMode = NormalCitation\n            , citationNoteNum = 0\n            , citationHash = 0\n            }\n        ]\n        [ Str \"citep:lee\" ]\n    , Str \",\"\n    , Space\n    , Cite\n        [ Citation\n            { citationId = \"kim\"\n            , citationPrefix = []\n            , citationSuffix = []\n            , citationMode = SuppressAuthor\n            , citationNoteNum = 0\n            , citationHash = 0\n            }\n        ]\n        [ Str \"citeyear:kim\" ]\n    , Str \"_\"\n    , Space\n    , Str \"(\"\n    , Cite\n        [ Citation\n            { citationId = \"ng\"\n            , citationPrefix = []\n            , citationSuffix = []\n            , citationMode = AuthorInText\n            , citationNoteNum = 0\n            , citationHash = 0\n            }\n        ]\n        [ Str \"citet*:&ng\" ]\n    , Str \").\"\n    ]\n]\n",
    ),
    (
        "title_takes_identifier",
        "#+TITLE: Field\n#+TITLE: Notes\n* Field Notes\n* Field\n",
        "[ Header\n    1\n    ( \"field-notes-1\" , [] , [] )\n    [ Str \"Field\" , Space , Str \"Notes\" ]\n, Header 1 ( \"field\" , [] , [] ) [ Str \"Field\" ]\n]\n",
    ),
    (
        "latex_math_environment_is_inline",
        "Energy\n\\begin{equation}\nE = mc^2\n\\end{equation}\nholds \\begin{align*}a\\end{align*}.\n\n\\begin{itemize}x\\end{itemize} and \\end{foo}.\n\n\\begin{gather}\n  a\n    b\n\\end{gather}\n",
        "[ Para\n    [ Str \"Energy\"\n    , SoftBreak\n    , RawInline\n        (Format \"latex\")\n        \"\\\\begin{equation}\\nE = mc^2\\n\\\\end{equation}\"\n    , SoftBreak\n    , Str \"holds\"\n    , Space\n    , RawInline (Format \"latex\") \"\\\\begin{align*}a\\\\end{align*}\"\n    , Str \".\"\n    ]\n, Para\n    [ Str \"\\\\begin{itemize}x\\\\end{itemize}\"\n    , Space\n    , Str \"and\"\n    , Space\n    , Str \"\\\\end{foo}.\"\n    ]\n, Para\n    [ RawInline\n        (Format \"latex\")\n        \"\\\\begin{gather}\\n  a\\n    b\\n\\\\end{gather}\"\n    ]\n]\n",
    ),
];

pub(super) const WRITER: &[(&str, &str, &[&str], &str, &str)] = &[
    (
        "smart_quotes_option",
        "org+smart_quotes-special_strings",
        &["-s"],
        "[Para [Quoted DoubleQuote [Str \"It’s\"], Space, Str \"–\", Space, Str \"ok\"]]",
        "#+options: ':t\n#+options: -:nil\n\n\"It’s\" – ok\n",
    ),
    (
        "note_stays_on_the_line",
        "org",
        &[],
        "[Para [Str \"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\", Space, Note [Para [Str \"b\"]], Space, Str \"c\"]]",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA [fn:1]\nc\n\n[fn:1] b\n",
    ),
    (
        "item_starts_with_block",
        "org",
        &[],
        "[BulletList [[CodeBlock (\"\",[],[]) \"x\"], [Plain [Str \"c\"]], [Plain [Str \"d\"], BulletList [[Para [Str \"e\"]]]], [Plain [Str \"f\"]]]]",
        "- \n  #+begin_example\n  x\n  #+end_example\n\n- c\n\n- d\n  - e\n\n- f\n",
    ),
    (
        "tags_fill_the_columns",
        "org",
        &["--columns=78"],
        "[Header 1 (\"\",[],[]) [Str \"Notes\", Space, Span (\"\",[\"tag\"],[(\"tag-name\",\"a\")]) [SmallCaps [Str \"a\"]], Str \" \", Span (\"\",[\"tag\"],[(\"tag-name\",\"b\")]) [SmallCaps [Str \"b\"]]]]",
        "* Notes                                                                  :a:b:\n",
    ),
    (
        "citation_suffixes",
        "org",
        &[],
        "[Para [Cite [Citation {citationId = \"a\", citationPrefix = [], citationSuffix = [Str \"p.\", Space, Str \"4\"], citationMode = AuthorInText, citationNoteNum = 0, citationHash = 0}, Citation {citationId = \"b\", citationPrefix = [], citationSuffix = [Str \",\", Space, Str \"chap.\", Space, Str \"2\"], citationMode = NormalCitation, citationNoteNum = 0, citationHash = 0}, Citation {citationId = \"c\", citationPrefix = [], citationSuffix = [Str \"and\", Space, Str \"more\"], citationMode = NormalCitation, citationNoteNum = 0, citationHash = 0}] []]]",
        "[cite/t:@a p. 4; @b chap. 2; @c , and more]\n",
    ),
    (
        "jupyter_cells",
        "org",
        &[],
        "[Div (\"\",[\"cell\",\"code\"],[(\"execution_count\",\"1\")]) [CodeBlock (\"\",[\"r\"],[]) \"1 + 1\", Div (\"\",[\"output\",\"execute_result\"],[]) [CodeBlock (\"\",[],[]) \"2\\n\\n3\"]], Div (\"\",[\"cell\",\"code\"],[]) [CodeBlock (\"\",[\"python\"],[]) \"x\", Div (\"\",[\"output\",\"stream\"],[]) [CodeBlock (\"\",[],[]) \"out\"]]]",
        "#+begin_src jupyter-R\n1 + 1\n#+end_src\n\n#+RESULTS:\n: 2\n: 3\n#+begin_src jupyter-python\nx\n#+end_src\n\n#+begin_example\nout\n#+end_example\n",
    ),
    (
        "blocks_after_plain_text",
        "org",
        &[],
        "[Plain [Str \"a\"], Table (\"\",[],[]) (Caption Nothing []) [(AlignDefault,ColWidthDefault)] (TableHead (\"\",[],[]) []) [TableBody (\"\",[],[]) (RowHeadColumns 0) [] [Row (\"\",[],[]) [Cell (\"\",[],[]) AlignDefault (RowSpan 1) (ColSpan 1) [Plain [Str \"x\"]]]]] (TableFoot (\"\",[],[]) []), Plain [Str \"b\"], Div (\"\",[\"note\"],[]) [Para [Str \"n\"]], Plain [Str \"c\"], Div (\"i\",[],[]) [Plain [Str \"d\"]]]",
        "a\n| x |\n\nb\n\n#+begin_note\nn\n#+end_note\n\nc\n\n<<i>>\nd\n",
    ),
];
