//! Regression cases of the clean-room languages: inputs written for the
//! black-box runs, with the tokens of the oracle (Pandoc 3.13). Each case
//! is the name, the classes, the inline flag, the code and the token dump:
//! one `class<TAB>text` line for each token (`--` for `Normal`, a tab as
//! `\t`, a backslash as `\\`), and an empty line after each source line.

use randoc_highlight::{Registry, SourceLine, TokenType};

const CASES: &[(&str, &str, bool, &str, &str)] = &[
    (
        r#"bash/02-control"#,
        r#"bash"#,
        false,
        r#"if [ -f "$f" ] && [[ $a == b* ]] || (( n > 1 )); then echo y; elif true; then :; else false; fi
for i in 1 2 3; do echo $i; done; while read -r line; do continue; done < file
until false; do break; done; select s in a b; do break; done
case "$x" in a|b) echo ab;; *) echo other;& esac
function f { return 0; }
g() { local x; }
[ -z "$a" -o -n "$b" ] && test -d x
! x; esac; in; }"#,
        r#"cf	if
--	 
bu	[
--	 
ot	-f
--	 
st	"
va	$f
st	"
--	 
bu	]
--	 
kw	&&
--	 
kw	[[
--	 
va	$a
--	 
ot	==
--	 b
pp	*
--	 
kw	]]
--	 
kw	||
--	 
kw	((
--	 
va	n
--	 
op	>
--	 
dv	1
--	 
kw	));
--	 
cf	then
--	 
bu	echo
--	 y
kw	;
--	 
cf	elif
--	 
fu	true
kw	;
--	 
cf	then
--	 
bu	:
kw	;
--	 
cf	else
--	 
fu	false
kw	;
--	 
cf	fi

cf	for
--	 i 
kw	in
--	 1 2 3
kw	;
--	 
cf	do
--	 
bu	echo
--	 
va	$i
kw	;
--	 
cf	done
kw	;
--	 
cf	while
--	 
bu	read
--	 
at	-r
--	 
va	line
kw	;
--	 
cf	do
--	 
cf	continue
kw	;
--	 
cf	done
--	 
op	<
--	 file

cf	until
--	 
fu	false
kw	;
--	 
cf	do
--	 
cf	break
kw	;
--	 
cf	done
kw	;
--	 
cf	select
--	 s 
kw	in
--	 a b
kw	;
--	 
cf	do
--	 
cf	break
kw	;
--	 
cf	done

cf	case
--	 
st	"
va	$x
st	"
--	 
kw	in
--	 
ss	a
kw	|
ss	b
kw	)
--	 
bu	echo
--	 ab
cf	;;
--	 
pp	*
kw	)
--	 
bu	echo
--	 other
cf	;&
--	 
cf	esac

kw	function
fu	 f
--	 
kw	{
--	 
cf	return
--	 
dv	0
kw	;
--	 
kw	}

fu	g()
--	 
kw	{
--	 
bu	local
--	 
va	x
kw	;
--	 
kw	}

bu	[
--	 
ot	-z
--	 
st	"
va	$a
st	"
--	 
ot	-o
--	 
ot	-n
--	 
st	"
va	$b
st	"
--	 
bu	]
--	 
kw	&&
--	 
bu	test
--	 
at	-d
--	 x

ot	! 
ex	x
kw	;
--	 
er	esac
kw	;
--	 
er	in
kw	;
--	 
er	}
"#,
    ),
    (
        r#"bash/03-heredoc-redirect"#,
        r#"bash"#,
        false,
        r#"cat <<EOF
heredoc $x line
EOF
cat <<-'EOT' > out
	literal $x
	EOT
cat << "Q" | wc
q $x
Q
ls -la --color=auto /tmp/*.txt ~/x ./a 2>&1 | grep -v foo >> log &
cmd 2>/dev/null &>all <<< "str" |& tee
a=$((1 + 2 * x)); (( i++ )); let "j = 1"
diff <(sort a) >(cat)
echo {a,b,c} {1..10} file?.txt [abc]*
time sleep 1; coproc foo; exec 3>&1
mycmd --opt=val -x arg; ./script.sh; /usr/bin/env python
sudo apt-get install -y curl && \
  rm -rf /x"#,
        r#"fu	cat
--	 
op	<<EOF

st	heredoc 
va	$x
st	 line

op	EOF

fu	cat
--	 
op	<<-'EOT'
--	 
op	>
--	 out

st	\tliteral $x

op	\tEOT

fu	cat
--	 
op	<< "Q"
--	 
kw	|
--	 
fu	wc

st	q $x

op	Q

fu	ls
--	 
at	-la
--	 
at	--color
op	=
--	auto /tmp/
pp	*
--	.txt ~/x ./a 
dv	2
op	>&
dv	1
--	 
kw	|
--	 
fu	grep
--	 
at	-v
--	 foo 
op	>>
--	 log 
kw	&

ex	cmd
--	 
dv	2
op	>
--	/dev/null 
op	&>
--	all 
op	<<<
--	 
st	"str"
--	 
kw	|&
--	 
fu	tee

va	a
op	=
va	$((
dv	1
--	 
op	+
--	 
dv	2
--	 
op	*
--	 
va	x))
kw	;
--	 
kw	((
--	 
va	i
op	++
--	 
kw	));
--	 
bu	let
--	 
st	"j = 1"

fu	diff
--	 
op	<(
fu	sort
--	 a
op	)
--	 
op	>(
fu	cat
op	)

bu	echo
--	 
dt	{a
op	,
dt	b
op	,
dt	c}
--	 
dt	{
dv	1
dt	..
dv	10
dt	}
--	 file
pp	?
--	.txt 
pp	[
ss	abc
pp	]*

bu	time
--	 sleep 1
kw	;
--	 
bu	coproc
--	 
ex	foo
kw	;
--	 
bu	exec
--	 
dv	3
op	>&
dv	1

ex	mycmd
--	 
at	--opt
op	=
--	val 
at	-x
--	 arg
kw	;
--	 
ex	./script.sh
kw	;
--	 
ex	/usr/bin/env
--	 python

fu	sudo
--	 apt-get install 
at	-y
--	 curl 
kw	&&
--	 
dt	\\

--	  
fu	rm
--	 
at	-rf
--	 /x
"#,
    ),
    (
        r#"bash/05-command-positions"#,
        r#"bash"#,
        false,
        r#"(cd x; foo)
{ ls; foo; }
x | foo; a || foo; b && foo; c & foo
if a; then foo; else foo; fi
for i in x; do foo; done
echo `foo` $(foo) <(foo)"#,
        r#"kw	(
bu	cd
--	 x
kw	;
--	 
ex	foo
kw	)

kw	{
--	 
fu	ls
kw	;
--	 
ex	foo
kw	;
--	 
kw	}

ex	x
--	 
kw	|
--	 
ex	foo
kw	;
--	 
ex	a
--	 
kw	||
--	 
ex	foo
kw	;
--	 
ex	b
--	 
kw	&&
--	 
ex	foo
kw	;
--	 
ex	c
--	 
kw	&
--	 
ex	foo

cf	if
--	 
ex	a
kw	;
--	 
cf	then
--	 
ex	foo
kw	;
--	 
cf	else
--	 
ex	foo
kw	;
--	 
cf	fi

cf	for
--	 i 
kw	in
--	 x
kw	;
--	 
cf	do
--	 
ex	foo
kw	;
--	 
cf	done

bu	echo
--	 
kw	`
ex	foo
kw	`
--	 
va	$(
ex	foo
va	)
--	 
op	<(
ex	foo
op	)
"#,
    ),
    (
        r#"dockerfile/01-bashoneline"#,
        r#"dockerfile"#,
        false,
        r#"RUN echo a; ls -la | grep x && cd /y || exit 1
RUN apt-get update && \
    apt-get install -y curl && \
    rm -rf /var/lib/apt/lists/*
RUN --mount=type=cache,target=/x make build
RUN ["executable", "param1"]
RUN if [ -f x ]; then echo "$HOME"; fi
RUN x=1 && echo ${x:-2} $(date) `pwd` 'q'
RUN for i in 1 2; do echo $i; done
CMD echo $A
ENTRYPOINT exec /usr/bin/app --flag"#,
        r#"kw	RUN
--	 
bu	echo
--	 a
kw	;
--	 
fu	ls
--	 
at	-la
--	 
kw	|
--	 
fu	grep
--	 x 
kw	&&
--	 
bu	cd
--	 /y 
kw	||
--	 
bu	exit
--	 1

kw	RUN
--	 
ex	apt-get
--	 update 
kw	&&
--	 
dt	\\

--	    
ex	apt-get
--	 install 
at	-y
--	 curl 
kw	&&
--	 
dt	\\

--	    
fu	rm
--	 
at	-rf
--	 /var/lib/apt/lists/
pp	*

kw	RUN
--	 
op	--mount=type=cache,target=/x
--	 
fu	make
--	 build

kw	RUN
--	 [
st	"executable"
--	, 
st	"param1"
--	]

kw	RUN
--	 
cf	if
--	 
bu	[
--	 
ot	-f
--	 x 
bu	]
kw	;
--	 
cf	then
--	 
bu	echo
--	 
st	"
va	$HOME
st	"
kw	;
--	 
cf	fi

kw	RUN
--	 
va	x
op	=
--	1 
kw	&&
--	 
bu	echo
--	 
va	${x
op	:-
--	2
va	}
--	 
va	$(
fu	date
va	)
--	 
kw	`
bu	pwd
kw	`
--	 
st	'q'

kw	RUN
--	 
cf	for
--	 i 
kw	in
--	 1 2
kw	;
--	 
cf	do
--	 
bu	echo
--	 
va	$i
kw	;
--	 
cf	done

kw	CMD
--	 
bu	echo
--	 
va	$A

kw	ENTRYPOINT
--	 
bu	exec
--	 /usr/bin/app 
at	--flag
"#,
    ),
    (
        r#"elixir/02-control-ops"#,
        r#"elixir"#,
        false,
        r#"case x do
  {:ok, v} -> v
  _ -> raise "err"
end
cond do x > 1 -> 1; true -> 2 end
with {:ok, a} <- f(), do: a
fn x -> x + 1 end; &Enum.count/1; &(&1 + &2)
try do: throw(1), rescue: e in RuntimeError -> e, catch: x -> x, after: :ok
receive do msg -> msg after 100 -> :timeout end
a = 1; b == c; d != e; f === g; h =~ i; j && k || l; not m; n and o or p; q <> r; s ++ t; u -- v; w in x
x |> y; a <|> b; a ~> b; a .. b; a..b//2; a ** b; a :: b; a -> b; a <- b; a \\ b; ^pin; a = b
x.(1); IO.puts("a"); Kernel.+(1, 2); unless a, do: b; for x <- l, into: %{}, do: x; reraise e, s"#,
        r#"kw	case
--	 x 
kw	do

--	  {
va	:ok
--	, v} 
er	->
--	 v

--	  
co	_
--	 
er	->
--	 
cf	raise
--	 
st	"err"

kw	end

cf	cond
--	 
kw	do
--	 x 
er	>
--	 
dv	1
--	 
er	->
--	 
dv	1
--	; 
va	true
--	 
er	->
--	 
dv	2
--	 
kw	end

kw	with
--	 {
va	:ok
--	, a} 
er	<-
--	 
fu	f
--	(), 
va	do:
--	 a

kw	fn
--	 x 
er	->
--	 x 
er	+
--	 
dv	1
--	 
kw	end
--	; 
er	&
--	Enum.count
er	/
dv	1
--	; 
er	&
--	(
er	&
dv	1
--	 
er	+
--	 
er	&
dv	2
--	)

cf	try
--	 
va	do:
--	 
cf	throw
--	(
dv	1
--	), 
va	rescue:
--	 e 
kw	in
--	 RuntimeError 
er	->
--	 e, 
va	catch:
--	 x 
er	->
--	 x, 
va	after:
--	 
va	:ok

kw	receive
--	 
kw	do
--	 msg 
er	->
--	 msg 
kw	after
--	 
dv	100
--	 
er	->
--	 
va	:timeout
--	 
kw	end

--	a 
er	=
--	 
dv	1
--	; b 
er	==
--	 c; d 
er	!=
--	 e; f 
er	===
--	 g; h 
er	=~
--	 i; j 
er	&&
--	 k 
er	||
--	 l; 
kw	not
--	 m; n 
kw	and
--	 o 
kw	or
--	 p; q 
er	<>
--	 r; s 
er	++
--	 t; u 
er	--
--	 v; w 
kw	in
--	 x

--	x 
er	|>
--	 y; a 
er	<|>
--	 b; a 
er	~>
--	 b; a 
op	..
--	 b; a
op	..
--	b
er	//
dv	2
--	; a 
er	**
--	 b; a 
op	::
--	 b; a 
er	->
--	 b; a 
er	<-
--	 b; a 
er	\\\\
--	 b; 
er	^
--	pin; a 
er	=
--	 b

--	x.(
dv	1
--	); IO.
fu	puts
--	(
st	"a"
--	); Kernel.
er	+
--	(
dv	1
--	, 
dv	2
--	); 
cf	unless
--	 a, 
va	do:
--	 b; 
kw	for
--	 x 
er	<-
--	 l, 
va	into:
--	 %{}, 
va	do:
--	 x; 
cf	reraise
--	 e, s
"#,
    ),
    (
        r#"elixireex/01-expression"#,
        r#"elixireex"#,
        false,
        r#"<%= @doc %> <%= @moduledoc "x" %> <%= @items |> length() %> <%= _x %> <%= :a %>
<%= if @user do %>Hi <%= @user.name %><% else %>Guest<% end %>
<%= for item <- @items do %><li><%= item.name %></li><% end %>
<%= "a #{b}" <> ~s(c) %> <%# comment %> <%% raw %>"#,
        r#"bu	<%=
--	 
fu	@doc
--	 
bu	%>
--	 
bu	<%=
--	 
fu	@moduledoc
--	 
st	"x"
--	 
bu	%>
--	 
bu	<%=
--	 
fu	@items
--	 
er	|>
--	 
fu	length
--	() 
bu	%>
--	 
bu	<%=
--	 
co	_x
--	 
bu	%>
--	 
bu	<%=
--	 
va	:a
--	 
bu	%>

bu	<%=
--	 
cf	if
--	 
fu	@user
--	 
kw	do
--	 
bu	%>
--	Hi 
bu	<%=
--	 
fu	@user
--	.name 
bu	%><%
--	 
cf	else
--	 
bu	%>
--	Guest
bu	<%
--	 
kw	end
--	 
bu	%>

bu	<%=
--	 
kw	for
--	 item 
er	<-
--	 
fu	@items
--	 
kw	do
--	 
bu	%>
--	<li>
bu	<%=
--	 item.name 
bu	%>
--	</li>
bu	<%
--	 
kw	end
--	 
bu	%>

bu	<%=
--	 
st	"a 
fu	#{
--	b
fu	}
st	"
--	 
er	<>
--	 
st	~s(c)
--	 
bu	%>
--	 
bu	<%
co	# comment %> <%% raw %>
"#,
    ),
    (
        r#"elixirheex/04-entityrefs-attr"#,
        r#"elixirheex"#,
        false,
        r#"<p a="x &amp; & y &#1; &bogus" b='q &lt;'>t &amp;</p>"#,
        r#"bu	<
kw	p
--	 a
op	=
st	"x 
dv	&amp;
st	 
er	&
st	 y 
dv	&#1;
st	 
er	&
st	bogus"
--	 b
op	=
st	'q &lt;'>t &amp;</p>
"#,
    ),
    (
        r#"html/02-tags-attrs"#,
        r#"html"#,
        false,
        r#"<foo> <my-el x=1> <svg:rect/> <DIV CLASS="a"> <Div> <a.b> <a_b> <a1> <_x> <1x> <:x> <a->
</p > < p> </ p> </> <> </p x> <p
  class="a"
  id=b>x</p>
<p class = "a" title= 'b' data-a=c/d>
<p "x"> <p x"y"> <p a<b> <p a&amp;b> <p ="x"> <p x=> <p x= > <p / >
<img src=a.png alt=x/> <p data-x="1"data-y="2"> <p x="a>b"> <p x='a"b'>
<p title="a
b">z</p>
<p	class="t">tab</p> <input type="checkbox" checked> <br />
a > b < c"#,
        r#"dt	<
kw	foo
dt	>
--	 
dt	<
kw	my-el
ot	 x
op	=
st	1
dt	>
--	 
dt	<
kw	svg
ot	:rect
dt	/>
--	 
dt	<
kw	DIV
ot	 CLASS
op	=
st	"a"
dt	>
--	 
dt	<
kw	Div
dt	>
--	 
dt	<
kw	a.b
dt	>
--	 
dt	<
kw	a_b
dt	>
--	 
dt	<
kw	a1
dt	>
--	 
dt	<
kw	_x
dt	>
--	 
er	<
--	1x> 
dt	<
kw	:x
dt	>
--	 
dt	<
kw	a-
dt	>

dt	</
kw	p
--	 
dt	>
--	 
er	<
--	 p> 
er	<
--	/ p> 
er	<
--	/> 
er	<
--	> 
dt	</
kw	p
--	 
er	x
dt	>
--	 
dt	<
kw	p

ot	  class
op	=
st	"a"

ot	  id
op	=
st	b
dt	>
--	x
dt	</
kw	p
dt	>

dt	<
kw	p
ot	 class 
op	=
--	 
st	"a"
ot	 title
op	=
--	 
st	'b'
ot	 data-a
op	=
st	c/d
dt	>

dt	<
kw	p
ot	 
er	"
ot	x
er	"
dt	>
--	 
dt	<
kw	p
ot	 x
er	"
ot	y
er	"
dt	>
--	 
dt	<
kw	p
ot	 a
er	<
ot	b
dt	>
--	 
dt	<
kw	p
ot	 a
er	&
ot	amp
er	;
ot	b
dt	>
--	 
dt	<
kw	p
ot	 
op	=
st	"x"
dt	>
--	 
dt	<
kw	p
ot	 x
op	=
dt	>
--	 
dt	<
kw	p
ot	 x
op	=
--	 
dt	>
--	 
dt	<
kw	p
ot	 
er	/
ot	 
dt	>

dt	<
kw	img
ot	 src
op	=
st	a.png
ot	 alt
op	=
st	x/
dt	>
--	 
dt	<
kw	p
ot	 data-x
op	=
st	"1"
ot	data-y
op	=
st	"2"
dt	>
--	 
dt	<
kw	p
ot	 x
op	=
st	"a>b"
dt	>
--	 
dt	<
kw	p
ot	 x
op	=
st	'a"b'
dt	>

dt	<
kw	p
ot	 title
op	=
st	"a

st	b"
dt	>
--	z
dt	</
kw	p
dt	>

dt	<
kw	p
ot	\tclass
op	=
st	"t"
dt	>
--	tab
dt	</
kw	p
dt	>
--	 
dt	<
kw	input
ot	 type
op	=
st	"checkbox"
ot	 checked
dt	>
--	 
dt	<
kw	br
ot	 
dt	/>

--	a > b 
er	<
--	 c
"#,
    ),
    (
        r#"html/03-entities"#,
        r#"html"#,
        false,
        r#"&amp; &#169; &#x41; &#X41; &nbsp &bogus; &AMP; &amp &lt;b&gt; &a1; &a1b;
&amp
&amp.x &amp&amp; &1; &A; &a; &x-y; &x.y; &x_y; &; & x
&#0; &#x; &#xa; &#xg; &#169 &#x41 &#12a; &#;
<a href="x?a=1&amp;b=2&c">y</a>"#,
        r#"dv	&amp;
--	 
dv	&#169;
--	 
dv	&#x41;
--	 
dv	&#X41;
--	 
dv	&nbsp
--	 
dv	&bogus;
--	 
dv	&AMP;
--	 
dv	&amp
--	 
dv	&lt;
--	b
dv	&gt;
--	 
dv	&a1;
--	 
dv	&a1b;

dv	&amp

dv	&amp
--	.x 
dv	&amp&amp;
--	 
er	&
--	1; 
er	&
--	A; 
er	&
--	a; 
er	&
--	x-y; 
er	&
--	x.y; 
er	&
--	x_y; 
er	&
--	; 
er	&
--	 x

dv	&#0;
--	 
er	&
--	#x; 
dv	&#xa;
--	 
er	&
--	#xg; 
er	&
--	#169 
er	&
--	#x41 
er	&
--	#12a; 
er	&
--	#;

dt	<
kw	a
ot	 href
op	=
st	"x?a=1
dv	&amp;
st	b=2
er	&
st	c"
dt	>
--	y
dt	</
kw	a
dt	>
"#,
    ),
    (
        r#"html/05-script"#,
        r#"html"#,
        false,
        r#"<script src="a.js" async></script>
<script>var s = "</script>"; x</script>
<script>
// </script> after
</script>
<SCRIPT>var a;</SCRIPT>
<script type="module">import x from "y";</script>
<script>/* a </script> b */</script>
<script type="application/json">{"a": 1}</script>
<script type = "a">y</script>
<script typex="a">y</script>
<script data-type="a">y</script>
<script type="text/html">var x = 1; <b>&amp;</b></script>
<script type='text/html'>var x = 1; <b>&amp;</b></script>
<script type=text/html>var x = 1;</script>
<script type="text/x-handlebars-template">var x = 1; <b>&amp;</b></script>
<script type="text/template">var x = 1; <b>&amp;</b></script>
<script>
if (a < b) { }
</script >
<script>x</script
>
<script>var a = `
</script>`;</script>
<script>
/* multi
line </script> */
</script>"#,
        r#"dt	<
kw	script
ot	 src
op	=
st	"a.js"
ot	 async
dt	></
kw	script
dt	>

dt	<
kw	script
dt	>
kw	var
--	 s 
op	=
--	 
st	"</script>"
op	;
--	 x
dt	</
kw	script
dt	>

dt	<
kw	script
dt	>

co	// 
dt	</
kw	script
dt	>
--	 after

dt	</
kw	script
dt	>

dt	<
kw	SCRIPT
dt	>
kw	var
--	 a
op	;
dt	</
kw	SCRIPT
dt	>

dt	<
kw	script
ot	 type=
st	"module"
dt	>
im	import
--	 x 
im	from
--	 
st	"y"
op	;
dt	</
kw	script
dt	>

dt	<
kw	script
dt	>
co	/* a </script> b */
dt	</
kw	script
dt	>

dt	<
kw	script
ot	 type=
st	"application/json"
dt	>
--	{
st	"a"
op	:
--	 
dv	1
--	}
dt	</
kw	script
dt	>

dt	<
kw	script
ot	 type
--	 
ot	=
--	 
st	"a"
dt	>
--	y
dt	</
kw	script
dt	>

dt	<
kw	script
ot	 typex
op	=
st	"a"
dt	>
--	y
dt	</
kw	script
dt	>

dt	<
kw	script
ot	 data-type
op	=
st	"a"
dt	>
--	y
dt	</
kw	script
dt	>

dt	<
kw	script
ot	 type=
st	"text/html"
dt	>
--	var x = 1; 
dt	<
kw	b
dt	>
dv	&amp;
dt	</
kw	b
dt	></
kw	script
dt	>

dt	<
kw	script
ot	 type=
st	'text/html'
dt	>
--	var x = 1; 
dt	<
kw	b
dt	>
dv	&amp;
dt	</
kw	b
dt	></
kw	script
dt	>

dt	<
kw	script
ot	 type=
st	text/html
dt	>
kw	var
--	 x 
op	=
--	 
dv	1
op	;
dt	</
kw	script
dt	>

dt	<
kw	script
ot	 type=
st	"text/x-handlebars-template"
dt	>
--	var x = 1; 
kw	<b>
dv	&amp;
kw	</b>
dt	</
kw	script
dt	>

dt	<
kw	script
ot	 type=
st	"text/template"
dt	>
kw	var
--	 x 
op	=
--	 
dv	1
op	;
--	 
op	<
--	b
op	>&
--	amp
op	;</
--	b
op	>
dt	</
kw	script
dt	>

dt	<
kw	script
dt	>

cf	if
--	 (a 
op	<
--	 b) { }

dt	</
kw	script
--	 
dt	>

dt	<
kw	script
dt	>
--	x
dt	</
kw	script

dt	>

dt	<
kw	script
dt	>
kw	var
--	 a 
op	=
--	 
vs	`

vs	</script>`
op	;
dt	</
kw	script
dt	>

dt	<
kw	script
dt	>

co	/* multi

co	line </script> */

dt	</
kw	script
dt	>
"#,
    ),
    (
        r#"html/06-style-rcdata"#,
        r#"html"#,
        false,
        r#"<style media="screen and (max-width: 10px)">a { color: red }</style>
<style>p::before { content: "</style>"; }</style>
<STYLE>p{}</STYLE>
<style lang="scss">$a: 1;</style>
<style type="text/css" media=all>a{}</style>
<style>
.x #y { margin: 0 auto !important; }
@media screen { a { b: c } }
</style>
<textarea>&amp; & <b>x</b></textarea>
<title>a <b> c &amp;</title>
<pre><b>x</b> &amp;</pre>"#,
        r#"dt	<
kw	style
ot	 media
op	=
st	"screen and (max-width: 10px)"
dt	>
--	a { 
kw	color
ch	:
--	 
cn	red
--	 }
dt	</
kw	style
dt	>

dt	<
kw	style
dt	>
--	p
in	::before
--	 { 
kw	content
ch	:
--	 
st	"</style>"
op	;
--	 }
dt	</
kw	style
dt	>

dt	<
kw	STYLE
dt	>
--	p{}
dt	</
kw	STYLE
dt	>

dt	<
kw	style
ot	 lang
op	=
st	"scss"
dt	>
--	$a
in	:
--	 1;
dt	</
kw	style
dt	>

dt	<
kw	style
ot	 type
op	=
st	"text/css"
ot	 media
op	=
st	all
dt	>
--	a{}
dt	</
kw	style
dt	>

dt	<
kw	style
dt	>

fu	.x
--	 
pp	#y
--	 { 
kw	margin
ch	:
--	 
dv	0
--	 
bu	auto
--	 
at	!important
op	;
--	 }

im	@media
--	 
an	screen
--	 { a { b
ch	:
--	 
dv	c 
--	} }

dt	</
kw	style
dt	>

dt	<
kw	textarea
dt	>
dv	&amp;
--	 
er	&
--	 <b>x</b>
dt	</
kw	textarea
dt	>

dt	<
kw	title
dt	>
--	a <b> c 
dv	&amp;
dt	</
kw	title
dt	>

dt	<
kw	pre
dt	><
kw	b
dt	>
--	x
dt	</
kw	b
dt	>
--	 
dv	&amp;
dt	</
kw	pre
dt	>
"#,
    ),
    (
        r#"html/07-upstream-5627"#,
        r#"html"#,
        false,
        r#"--><!--<script>alert('Escaped!')</script>"#,
        r#"--	-->
co	<!--<script>alert('Escaped!')</script>
"#,
    ),
    (
        r#"html/08-upstream-5627b"#,
        r#"html"#,
        false,
        r#"Something"#,
        r#"--	Something
"#,
    ),
    (
        r#"ini/01-document"#,
        r#"ini"#,
        false,
        r#"; comment
# hash comment
[section]
key = value
num=1
flt = 1.5
b = true
c = off
quoted = "str"
spaced key = v w
[sec.sub]
k:v
 indented = x
novalue
key = value ; trailing
key2 = value # trailing
[bad"#,
        r#"co	; comment

co	# hash comment

kw	[section]

dt	key 
ot	=
st	 value

dt	num
ot	=
dv	1

dt	flt 
ot	=
st	 
fl	1.5

dt	b 
ot	=
st	 
kw	true

dt	c 
ot	=
st	 
kw	off

dt	quoted 
ot	=
st	 "str"

dt	spaced key 
ot	=
st	 v w

kw	[sec.sub]

dt	k:v

dt	 indented 
ot	=
st	 x

dt	novalue

dt	key 
ot	=
st	 value ; trailing

dt	key2 
ot	=
st	 value # trailing

dt	[bad
"#,
    ),
    (
        r#"java/01-class"#,
        r#"java"#,
        false,
        r#"package com.example.app;
import java.util.List;
import static java.lang.Math.*;
/** Javadoc {@link X} @param a the a @return r */
@Override @SuppressWarnings("x")
public final class MyClass<T extends Comparable<T>> extends Base implements Runnable, java.io.Serializable {
  private static final int X = 0x1F + 077 + 0b101 + 1_000 + 10L + 1.5f + 2.0d + 1e3 + 'c' + '\n';
  protected volatile transient long y; native synchronized void m() throws Exception;
  public abstract strictfp double z(final String s, int... a);
  // line comment TODO
  /* block */
  String s = "str \" \t" + """
    text block
    """;
  void f() { if (a) return; else { for (int i = 0; i < 10; i++) { continue; } } while (true) break; do {} while (false);
    switch (x) { case 1 -> y; default: z; } try { throw new RuntimeException(); } catch (Exception | Error e) {} finally {}
    assert x : "m"; this.x = super.y; Object o = null; boolean b = true || false; var v = new int[]{1}; x instanceof String s2;
    System.out.println(o.toString()); Integer.parseInt("1"); List<String> l = new ArrayList<>(); Thread t; Math.max(1, 2);
    yield 1; record R(int a) {} sealed interface I permits A {} enum E { A, B } @interface Ann {} goto; const;
    x += 1; x >>>= 2; x = a ? b : c; lambda = (a, b) -> a + b; ref = String::valueOf;
  }
}"#,
        r#"kw	package
im	 com
op	.
im	example
op	.
im	app
op	;

kw	import
--	 
im	java
op	.
im	util
op	.
im	List
op	;

kw	import
--	 
kw	static
im	 java
op	.
im	lang
op	.
im	Math
op	.*;

co	/**
--	 Javadoc 
an	{@link
--	 X
an	}
co	 
an	@param
co	 
an	a
co	 
--	the a 
an	@return
--	 r 
co	*/

at	@Override
--	 
at	@SuppressWarnings
op	(
st	"x"
op	)

kw	public
--	 
dt	final
--	 
kw	class
--	 MyClass
op	<
--	T 
kw	extends
--	 
bu	Comparable
op	<
--	T
op	>>
--	 
kw	extends
--	 Base 
kw	implements
--	 
bu	Runnable
op	,
--	 java
op	.
fu	io
op	.
fu	Serializable
--	 
op	{

--	  
kw	private
--	 
dt	static
--	 
dt	final
--	 
dt	int
--	 X 
op	=
--	 
bn	0x1F
--	 
op	+
--	 
bn	077
--	 
op	+
--	 
bn	0b101
--	 
op	+
--	 
dv	1_000
--	 
op	+
--	 
dv	10L
--	 
op	+
--	 
fl	1.5f
--	 
op	+
--	 
fl	2.0d
--	 
op	+
--	 
fl	1e3
--	 
op	+
--	 
ch	'c'
--	 
op	+
--	 
ch	'\\n'
op	;

--	  
kw	protected
--	 
kw	volatile
--	 
kw	transient
--	 
dt	long
--	 y
op	;
--	 
kw	native
--	 
kw	synchronized
--	 
dt	void
--	 
fu	m
op	()
--	 
kw	throws
--	 
bu	Exception
op	;

--	  
kw	public
--	 
kw	abstract
--	 
kw	strictfp
--	 
dt	double
--	 
fu	z
op	(
dt	final
--	 
bu	String
--	 s
op	,
--	 
dt	int
kw	...
--	 a
op	);

--	  
co	// line comment 
al	TODO

--	  
co	/* block */

--	  
bu	String
--	 s 
op	=
--	 
st	"str 
sc	\\"
st	 
sc	\\t
st	"
--	 
op	+
--	 
st	"""

st	    text block

st	    """
op	;

--	  
dt	void
--	 
fu	f
op	()
--	 
op	{
--	 
cf	if
--	 
op	(
--	a
op	)
--	 
cf	return
op	;
--	 
cf	else
--	 
op	{
--	 
cf	for
--	 
op	(
dt	int
--	 i 
op	=
--	 
dv	0
op	;
--	 i 
op	<
--	 
dv	10
op	;
--	 i
op	++)
--	 
op	{
--	 
cf	continue
op	;
--	 
op	}
--	 
op	}
--	 
cf	while
--	 
op	(
kw	true
op	)
--	 
cf	break
op	;
--	 
cf	do
--	 
op	{}
--	 
cf	while
--	 
op	(
kw	false
op	);

--	    
cf	switch
--	 
op	(
--	x
op	)
--	 
op	{
--	 
cf	case
--	 
dv	1
--	 
op	->
--	 y
op	;
--	 
kw	default
op	:
--	 z
op	;
--	 
op	}
--	 
cf	try
--	 
op	{
--	 
cf	throw
--	 
kw	new
--	 
bu	RuntimeException
op	();
--	 
op	}
--	 
cf	catch
--	 
op	(
bu	Exception
--	 
op	|
--	 
bu	Error
--	 e
op	)
--	 
op	{}
--	 
cf	finally
--	 
op	{}

--	    
cf	assert
--	 x 
op	:
--	 
st	"m"
op	;
--	 
kw	this
op	.
fu	x
--	 
op	=
--	 
kw	super
op	.
fu	y
op	;
--	 
bu	Object
--	 o 
op	=
--	 
kw	null
op	;
--	 
dt	boolean
--	 b 
op	=
--	 
kw	true
--	 
op	||
--	 
kw	false
op	;
--	 
dt	var
--	 v 
op	=
--	 
kw	new
--	 
dt	int
op	[]{
dv	1
op	};
--	 x 
kw	instanceof
--	 
bu	String
--	 s2
op	;

--	    
bu	System
op	.
fu	out
op	.
fu	println
op	(
--	o
op	.
fu	toString
op	());
--	 
bu	Integer
op	.
fu	parseInt
op	(
st	"1"
op	);
--	 
bu	List
op	<
bu	String
op	>
--	 l 
op	=
--	 
kw	new
--	 
bu	ArrayList
op	<>();
--	 
bu	Thread
--	 t
op	;
--	 
bu	Math
op	.
fu	max
op	(
dv	1
op	,
--	 
dv	2
op	);

--	    
cf	yield
--	 
dv	1
op	;
--	 
kw	record
--	 
fu	R
op	(
dt	int
--	 a
op	)
--	 
op	{}
--	 
kw	sealed
--	 
kw	interface
--	 I 
kw	permits
--	 A 
op	{}
--	 
kw	enum
--	 E 
op	{
--	 A
op	,
--	 B 
op	}
--	 
at	@interface
--	 Ann 
op	{}
--	 
cf	goto
op	;
--	 
dt	const
op	;

--	    x 
op	+=
--	 
dv	1
op	;
--	 x 
op	>>>=
--	 
dv	2
op	;
--	 x 
op	=
--	 a 
op	?
--	 b 
op	:
--	 c
op	;
--	 lambda 
op	=
--	 
op	(
--	a
op	,
--	 b
op	)
--	 
op	->
--	 a 
op	+
--	 b
op	;
--	 ref 
op	=
--	 
bu	String
op	::
--	valueOf
op	;

--	  
op	}

op	}
"#,
    ),
    (
        r#"java/02-upstream-3534"#,
        r#"java"#,
        true,
        r#"public class MyClass"#,
        r#"kw	public
--	 
kw	class
--	 MyClass
"#,
    ),
    (
        r#"json/01-document"#,
        r#"json"#,
        false,
        r#"{
  "key": "value \" \n \/ \x",
  "num": 1, "neg": -2, "flt": 1.5, "exp": 1e10, "e2": -1.5E-3, "zero": 0, "lead": 01,
  "t": true, "f": false, "n": null, "T": True,
  "arr": [1, "a", {"b": [true]}, []],
  "obj": {}, "nested": {"x": {"y": 1}}
  // comment
  /* block */
  'single': 'q',
  unquoted: 1
}
[1, 2]
"top""#,
        r#"fu	{

--	  
dt	"key"
fu	:
--	 
st	"value 
ch	\\"
st	 
ch	\\n
st	 
ch	\\/
st	 
er	\\x
st	"
fu	,

--	  
dt	"num"
fu	:
--	 
dv	1
fu	,
--	 
dt	"neg"
fu	:
--	 
dv	-2
fu	,
--	 
dt	"flt"
fu	:
--	 
fl	1.5
fu	,
--	 
dt	"exp"
fu	:
--	 
dv	1e10
fu	,
--	 
dt	"e2"
fu	:
--	 
fl	-1.5E-3
fu	,
--	 
dt	"zero"
fu	:
--	 
dv	0
fu	,
--	 
dt	"lead"
fu	:
--	 
er	01
fu	,

--	  
dt	"t"
fu	:
--	 
kw	true
fu	,
--	 
dt	"f"
fu	:
--	 
kw	false
fu	,
--	 
dt	"n"
fu	:
--	 
kw	null
fu	,
--	 
dt	"T"
fu	:
--	 
er	True
fu	,

--	  
dt	"arr"
fu	:
--	 
ot	[
dv	1
ot	,
--	 
st	"a"
ot	,
--	 
fu	{
dt	"b"
fu	:
--	 
ot	[
kw	true
ot	]
fu	}
ot	,
--	 
ot	[]]
fu	,

--	  
dt	"obj"
fu	:
--	 
fu	{},
--	 
dt	"nested"
fu	:
--	 
fu	{
dt	"x"
fu	:
--	 
fu	{
dt	"y"
fu	:
--	 
dv	1
fu	}}

--	  
er	//
--	 
er	comment

--	  
er	/*
--	 
er	block
--	 
er	*/

--	  
er	'single':
--	 
er	'q'
fu	,

--	  
er	unquoted
fu	:
--	 
dv	1

fu	}

ot	[
dv	1
ot	,
--	 
dv	2
ot	]

st	"top"
"#,
    ),
    (
        r#"latex/01-document"#,
        r#"latex"#,
        false,
        r#"\documentclass[a4paper]{article}
\usepackage{amsmath,graphicx}
% comment TODO
\begin{document}
\section{Intro}\label{sec:intro}
Text \emph{em} \textbf{bold} \cite{key} \ref{sec:intro} \\ \& \% \$ \{ ~ -- ---
Inline $x^2 + \alpha$ and \(y\) and display \[ z = 1 \] and $$w$$.
\begin{equation}
  E = mc^2 \label{eq}
\end{equation}
\begin{align*} a &= b \end{align*}
\begin{verbatim}
raw \text
\end{verbatim}
\verb|x \y| \verb+z+
\newcommand{\foo}[1]{#1}
\def\bar{x}
\item \includegraphics[width=\linewidth]{a.png}
\begin{lstlisting}
code
\end{lstlisting}
\begin{minted}{python}
x = 1
\end{minted}
\begin{comment}
hidden
\end{comment}
\end{document}"#,
        r#"bu	\\documentclass
--	[a4paper]{
ex	article
--	}

bu	\\usepackage
--	{
ex	amsmath,graphicx
--	}

co	% comment 
al	TODO

kw	\\begin
--	{
ex	document
--	}

kw	\\section
--	{Intro}
kw	\\label
--	{
ex	sec:intro
--	}

--	Text 
fu	\\emph
--	{em} 
fu	\\textbf
--	{bold} 
kw	\\cite
--	{
ex	key
--	} 
kw	\\ref
--	{
ex	sec:intro
--	} 
fu	\\\\
--	 
fu	\\&
--	 
fu	\\%
--	 
fu	\\$
--	 
fu	\\{
--	 ~ -- ---

--	Inline 
ss	$x^2 + 
sc	\\alpha
ss	$
--	 and 
ss	\\(y\\)
--	 and display 
ss	\\[ z = 1 \\]
--	 and 
ss	$$w$$
--	.

kw	\\begin
--	{
ex	equation
--	}

ss	  E = mc^2 
sc	\\label
ss	{eq}

kw	\\end
--	{
ex	equation
--	}

kw	\\begin
--	{
ex	align*
--	}
ss	 a &= b 
kw	\\end
--	{
ex	align*
--	}

kw	\\begin
--	{
ex	verbatim
--	}

vs	raw \\text

kw	\\end
--	{
ex	verbatim
--	}

fu	\\verb
--	|x 
fu	\\y
--	| 
fu	\\verb+
--	z+

fu	\\newcommand
--	{
ex	\\foo
--	}[1]{#1}

fu	\\def\\bar
--	{x}

fu	\\item
--	 
bu	\\includegraphics
--	[width=
fu	\\linewidth
--	]{
ex	a.png
--	}

kw	\\begin
--	{
ex	lstlisting
--	}

vs	code

kw	\\end
--	{
ex	lstlisting
--	}

kw	\\begin
--	{
ex	minted
--	}{python}

--	x 
op	=
--	 
dv	1

kw	\\end
--	{
ex	minted
--	}

kw	\\begin
--	{
ex	comment
--	}

co	hidden

kw	\\end
--	{
ex	comment
--	}

kw	\\end
--	{
ex	document
--	}
"#,
    ),
    (
        r#"markdown/01-document"#,
        r#"markdown"#,
        false,
        r#"# Heading 1
## Heading 2 ##
Setext
======
Para *em* _em_ **strong** __strong__ ***both*** `code` ``co`de`` ~~strike~~ <http://auto.link> [link](http://x "t") [ref][r] ![img](a.png) <b>html</b> &amp; \* escaped
> quote *em*
> > nested
- item
* item2
+ item3
1. one
2) two
- [ ] task
    indented code
```python
x = 1
```
~~~
tilde
~~~
---
***
[r]: http://x.y "Title"
<!-- comment -->
| a | b |
|---|---|
[^1]: footnote
$math$
- **item** *x*"#,
        r#"fu	# Heading 1

fu	## Heading 2 ##

--	Setext

fu	======

--	Para *em* _em_ **strong** __strong__ ***both*** 
in	`code`
--	 
in	``co`de``
--	 ~~strike~~ 
ot	<http://auto.link>
--	 
co	[
ot	link
co	](http://x "t")
--	 
co	[
ot	ref
co	][r]
--	 
al	![img](a.png)
--	 
dt	<
kw	b
dt	>
--	html
dt	</
kw	b
dt	>
--	 
dv	&amp;
--	 
sc	\\*
--	 escaped

at	> quote *em*

at	> > nested

ss	- 
--	item

ss	* 
--	item2

ss	+ 
--	item3

ss	1. 
--	one

--	2) two

ss	- 
va	[ ]
--	 task

--	    indented code

in	```python

--	x 
op	=
--	 
dv	1

in	```

in	~~~

in	tilde

in	~~~

--	---

--	***

ot	[r]: http://x.y "Title"

co	<!-- comment -->

pp	|
--	 a 
pp	|
--	 b 
pp	|

pp	|---|---|

ot	[^1]: 
--	footnote

--	$math$

ss	- 
--	**item** *x*
"#,
    ),
    (
        r#"markdown/02-metadata"#,
        r#"markdown"#,
        false,
        r#"Title: x
Author: y

Later: z"#,
        r#"an	Title:
co	 x

an	Author:
co	 y


--	Later: z
"#,
    ),
    (
        r#"mermaid/01-frontmatter-lvl0text"#,
        r#"mermaid"#,
        false,
        r#"---
just text
title: T
- x
  y: z
---
flowchart TD"#,
        r#"co	---

at	just text

fu	title
kw	:
at	 T

kw	- 
at	x

at	  
fu	y
kw	:
at	 z

co	---

kw	flowchart
--	 
cn	TD
"#,
    ),
    (
        r#"mustache/01-css-rule-selector"#,
        r#"mustache"#,
        false,
        r#"<style>
p { color: red; margin: 0 auto !important; b: c d }
.a #b:hover::after, [href="x" i] > c { content: "a\"b"; width: calc(100% - 2px) }
@media screen and (max-width: 600px) { a { color: {{c}} } }
/* comment */ a { b: {{x}} }
{{#if a}} .x { } {{/if}}
a[x="{{y}}"] { }
</style>"#,
        r#"kw	<style>

--	p { 
kw	color
ch	:
--	 
cn	red
op	;
--	 
kw	margin
ch	:
--	 
dv	0
--	 
bu	auto
--	 
at	!important
op	;
--	 b
ch	:
--	 
dv	c d 
--	}

fu	.a
--	 
pp	#b
in	:hover::after
op	,
--	 
ex	[
ss	href
op	=
st	"x" 
op	i
ex	]
--	 
op	>
--	 c { 
kw	content
ch	:
--	 
st	"a
sc	\\"
st	b"
op	;
--	 
kw	width
ch	:
--	 
fu	calc(
dv	100
dt	%
--	 
op	-
--	 
dv	2
dt	px
fu	)
--	 }

im	@media
--	 
an	screen
--	 
kw	and
--	 
fu	(
kw	max-width
ch	:
--	 
dv	600
dt	px
fu	)
--	 { a { 
kw	color
ch	:
--	 {{
dv	c
--	}} } }

co	/* comment */
--	 a { b
ch	:
--	 {{
dv	x
--	}} }

fu	{{#if
--	 a
fu	}}
--	 
fu	.x
--	 { } 
fu	{{/if}}

--	a
ex	[
ss	x
op	=
st	"{{y}}"
ex	]
--	 { }

kw	</style>
"#,
    ),
    (
        r#"out-of-scope/90-escapes-statement-start"#,
        r#"python"#,
        false,
        r#"r'a.b*c+d?e[f]g(h)|i^j$k{1,2}\d\w\s\\ \. \q'
'a.b*c \. \d \q \a \b \f \v \0 \x41 \N{DASH} A \U00000041 \' \" \\ \
x'
b'\. \x41 \N{x}'
R'x.y' rb'x.y' br'x.y' u'\.' f'{x}\.' rf'{x}.y'
"a\.b" r"a.b" """a\.b""" r"""a.b""""#,
        r#"co	r'a.b*c+d?e[f]g(h)|i^j$k{1,2}
er	\\
co	d
er	\\
co	w
er	\\
co	s
ch	\\\\
co	 
er	\\
co	. 
er	\\
co	q'

co	'a.b*c 
er	\\
co	. 
er	\\
co	d 
er	\\
co	q 
ch	\\a
co	 
ch	\\b
co	 
ch	\\f
co	 
ch	\\v
co	 
ch	\\0
co	 
ch	\\x41
co	 
ch	\\N{DASH}
co	 A 
ch	\\U00000041
co	 
ch	\\'
co	 
ch	\\"
co	 
ch	\\\\
co	 
op	\\

co	x'

st	b'
er	\\
st	. 
ch	\\x41
st	 
er	\\
st	N{x}'

co	R'x.y'
--	 
vs	rb'x
dv	.
vs	y'
--	 
vs	br'x
dv	.
vs	y'
--	 
st	u'
er	\\
st	.'
--	 
ss	f'
sc	{
--	x
sc	}
er	\\
ss	.'
--	 
vs	rf'
sc	{
--	x
sc	}
dv	.
vs	y'

co	"a
er	\\
co	.b"
--	 
vs	r"a
dv	.
vs	b"
--	 
st	"""a
er	\\
st	.b"""
--	 
vs	r"""a
dv	.
vs	b"""
"#,
    ),
    (
        r#"out-of-scope/91-escapes-expression"#,
        r#"python"#,
        false,
        r#"x = r'a.b*c+d?e[f]g(h)|i^j$k{1,2}\d\w\s\\ \. \q \( \[ \* \n'
x = 'a.b*c \. \d \q \a \b \f \v \0 \x41 \N{DASH} \U00000041 \' \" \\ \( \8 \777'
x = b'\. \x41 \N{x} A'
x = f'\. {y}' + rf'\. .{y}*' + Rb'a.b' + u'\d'"#,
        r#"--	x 
op	=
--	 
vs	r'a
dv	.
vs	b
op	*
vs	c
op	+
vs	d
op	?
vs	e
pp	[f]
vs	g
kw	(
vs	h
kw	)
cf	|
vs	i
dv	^
vs	j
dv	$
vs	k
op	{1,2}
dv	\\d\\w\\s
ch	\\\\
vs	 
ch	\\.
vs	 
er	\\
vs	q 
ch	\\(
vs	 
ch	\\[
vs	 
ch	\\*
vs	 
ch	\\n
vs	'

--	x 
op	=
--	 
st	'a.b*c 
er	\\
st	. 
er	\\
st	d 
er	\\
st	q 
ch	\\a
st	 
ch	\\b
st	 
ch	\\f
st	 
ch	\\v
st	 
ch	\\0
st	 
ch	\\x41
st	 
ch	\\N{DASH}
st	 
ch	\\U00000041
st	 
ch	\\'
st	 
ch	\\"
st	 
ch	\\\\
st	 
er	\\
st	( 
er	\\
st	8 
ch	\\777
st	'

--	x 
op	=
--	 
st	b'
er	\\
st	. 
ch	\\x41
st	 
er	\\
st	N{x} A'

--	x 
op	=
--	 
ss	f'
er	\\
ss	. 
sc	{
--	y
sc	}
ss	'
--	 
op	+
--	 
vs	rf'
ch	\\.
vs	 
dv	.
sc	{
--	y
sc	}
op	*
vs	'
--	 
op	+
--	 
vs	Rb'a.b'
--	 
op	+
--	 
st	u'
er	\\
st	d'
"#,
    ),
    (
        r#"r/01-level0"#,
        r#"r"#,
        false,
        r#"# comment
x <- c(1, 2L, 3.5, 1e3, 0x1F, 1i, -1) ; y <<- 2; 3 -> z; w = 4
f <- function(a, b = 2, ...) { if (a > b) return(a) else NULL }
for (i in seq_len(10)) { next; break }; while (TRUE) {}; repeat {}
NA NA_integer_ NA_real_ NA_character_ TRUE FALSE T F NULL Inf NaN
"str \n \"q\"" 'single' r"(raw)" `back tick`
x %in% y; x %>% f(); a |> g(); x$name; x@slot; x[[1]]; x[1, ]; a::b; a:::c
a & b | c && d || !e; a == b; a != b; a <= b; a >= b; a + b - c * d / e ^ f; ~ x; ?help
library(stats); print(x); paste0("a", "b"); lapply(l, function(x) x); invisible(x)
mean(x, na.rm = TRUE); plot(x, main = "t")
\(x) x + 1
print; library; c; is.null(x); x.y_z(1); .hidden(1); f (x); `my fun`(1); 1.5e3; 1e-3; .5; 5.; 1L; 1.5L
x[a = 1]; f(a == 1); g(a=1); h(`a b` = 2); k("a" = 3); l(a
= 1)
'multi
line'
# TODO note
x = list(a = 1, b = c(d = 2))"#,
        r#"co	# comment

--	x 
ot	<-
--	 
fu	c
--	(
dv	1
--	, 
dv	2
dt	L
--	, 
fl	3.5
--	, 
dv	1
--	e3, 
dv	0
--	x1F, 
dv	1
dt	i
--	, 
sc	-
dv	1
--	) ; y 
ot	<<-
--	 
dv	2
--	; 
dv	3
--	 
ot	->
--	 z; w 
ot	=
--	 
dv	4

--	f 
ot	<-
--	 
cf	function
--	(a, 
at	b =
--	 
dv	2
--	, ...) { 
cf	if
--	 (a 
sc	>
--	 b) 
fu	return
--	(a) 
cf	else
--	 
cn	NULL
--	 }

cf	for
--	 (i 
cf	in
--	 
fu	seq_len
--	(
dv	10
--	)) { 
cf	next
--	; 
cf	break
--	 }; 
cf	while
--	 (
cn	TRUE
--	) {}; 
cf	repeat
--	 {}

cn	NA
--	 
cn	NA_integer_
--	 
cn	NA_real_
--	 
cn	NA_character_
--	 
cn	TRUE
--	 
cn	FALSE
--	 T F 
cn	NULL
--	 
cn	Inf
--	 
cn	NaN

st	"str 
sc	\\n
st	 
sc	\\"
st	q
sc	\\"
st	"
--	 
st	'single'
--	 r
st	"(raw)"
--	 
st	`
at	back tick
st	`

--	x 
sc	%in%
--	 y; x 
sc	%>%
--	 
fu	f
--	(); a 
sc	|>
--	 
fu	g
--	(); x
sc	$
--	name; x
sc	@
--	slot; x[[
dv	1
--	]]; x[
dv	1
--	, ]; a
sc	::
--	b; a
sc	:::
--	c

--	a 
sc	&
--	 b 
sc	|
--	 c 
sc	&&
--	 d 
sc	||
--	 
sc	!
--	e; a 
sc	==
--	 b; a 
sc	!=
--	 b; a 
sc	<=
--	 b; a 
sc	>=
--	 b; a 
sc	+
--	 b 
sc	-
--	 c 
sc	*
--	 d 
sc	/
--	 e 
sc	^
--	 f; 
sc	~
--	 x; ?help

fu	library
--	(stats); 
fu	print
--	(x); 
fu	paste0
--	(
st	"a"
--	, 
st	"b"
--	); 
fu	lapply
--	(l, 
cf	function
--	(x) x); 
fu	invisible
--	(x)

fu	mean
--	(x, 
at	na.rm =
--	 
cn	TRUE
--	); 
fu	plot
--	(x, 
at	main =
--	 
st	"t"
--	)

--	\\(x) x 
sc	+
--	 
dv	1

--	print; library; c; 
fu	is.null
--	(x); 
fu	x.y_z
--	(
dv	1
--	); 
fu	.hidden
--	(
dv	1
--	); 
fu	f
--	 (x); 
st	`
at	my fun
st	`
--	(
dv	1
--	); 
fl	1.5e3
--	; 
dv	1
--	e
sc	-
dv	3
--	; 
fl	.5
--	; 
fl	5.
--	; 
dv	1
dt	L
--	; 
fl	1.5
--	L

--	x[a 
ot	=
--	 
dv	1
--	]; 
fu	f
--	(a 
sc	==
--	 
dv	1
--	); 
fu	g
--	(
at	a=
dv	1
--	); 
fu	h
--	(
st	`
at	a b
st	`
--	 
ot	=
--	 
dv	2
--	); 
fu	k
--	(
st	"a"
--	 
ot	=
--	 
dv	3
--	); 
fu	l
--	(a

ot	=
--	 
dv	1
--	)

st	'multi

st	line'

co	# 
al	TODO
co	 note

--	x 
ot	=
--	 
fu	list
--	(
at	a =
--	 
dv	1
--	, 
at	b =
--	 
fu	c
--	(
at	d =
--	 
dv	2
--	))
"#,
    ),
    (
        r#"rdoc/01-level0-embed"#,
        r#"rdoc"#,
        false,
        r#"\examples{
if (x) { y <- 1 }
f <- function(a = 1) {
  "s" # c
}
x[[1]] %in% c(TRUE, NA)
}
\usage{f(x, na.rm = FALSE)}
text after"#,
        r#"cf	\\examples
op	{

cf	if
--	 (x) 
op	{
--	 y 
ot	<-
--	 
dv	1
--	 
op	}

--	f 
ot	<-
--	 
cf	function
--	(
at	a =
--	 
dv	1
--	) 
op	{

--	  
st	"s"
--	 
co	# c

op	}

--	x[[
dv	1
--	]] 
co	%in% c(TRUE, NA)

op	}

cf	\\usage
op	{
fu	f
--	(x, 
at	na.rm =
--	 
cn	FALSE
--	)
op	}

--	text after
"#,
    ),
    (
        r#"rst/01-inline-and-blocks"#,
        r#"rst"#,
        false,
        r#"Title
=====

Paragraph with *emphasis*, **strong**, ``literal``, `interpreted`, `link <http://x>`_, ref_, `phrase ref`_, [1]_, [#]_, [CIT]_, |sub|, :role:`text`, :ref:`target`, `text`:role:, http://example.com, _`inline target`.

.. note:: Admonition text
   continued

.. code-block:: python

   x = 1

.. _target:

.. [1] Footnote

.. |sub| replace:: text

.. comment line
   more comment

..

* bullet
1. enum

:field: value

::

    literal block

Paragraph::

    literal"#,
        r#"--	Title

--	=====


--	Paragraph with *emphasis*, **strong**, 
dt	``literal``
--	, 
ot	`interpreted`, `link <http://x>`_
--	, 
ot	ref_
--	, 
ot	`phrase ref`_
--	, 
ot	[1]_
--	, [#]_, 
ot	[CIT]_
--	, 
fu	|sub|
--	, 
kw	:role:
dv	`text`
--	, 
kw	:ref:
dv	`target`
--	, 
dv	`text`
kw	:role:
--	, 
ot	http://example.com
--	, 
fu	_`inline target`
--	.


dt	.. note:: 
--	Admonition text

--	   continued


dt	.. code-block:: python


dt	   x = 1


dt	.. _target:


dt	.. [1] 
--	Footnote


dt	.. |sub| replace:: 
--	text


co	.. comment line

dt	   
co	more comment


co	..


--	* bullet

--	1. enum


fu	:field:
--	 value


dt	::


dt	    literal block


--	Paragraph
dt	::


dt	    literal
"#,
    ),
    (
        r#"ruby/01-program"#,
        r#"ruby"#,
        false,
        r#"require 'json'
require_relative "x"
module M; class C < B; include Enumerable; extend Forwardable
  attr_accessor :a, :b; attr_reader :c
  CONST = 1
  @ivar = 2; @@cvar = 3; $gvar = 4; $0; $stdout
  def initialize(a, b = 1, *args, key: 2, **opts, &blk)
    super; self; nil; true; false; __FILE__; __LINE__; __method__
  end
  def self.m = 1
  def method?; end; def method!; end; def ==(o); end
end; end
x = [1, 2.5, 1e3, 0x1F, 0b101, 0o17, 017, 1_000, 3r, 2i, ?a, ?\n]
h = { a: 1, "b" => 2, :c => 3, d: :sym, "e": 4 }
s = "dq #{x + 1} \n \t" + 'sq #{x} \'' + %q(pct q) + %Q[pct Q #{y}] + %w[a b] + %i[c d] + `cmd`
r = /re(gex)?\d+/i; r2 = %r{a/b}x
if x then y elsif z then w else v end; unless a; end; while b do end; until c; end
case x when 1 then :a when 2..3 then :b else :c end
for i in 1..10 do next; break; redo; retry; end
begin; raise "e"; rescue StandardError => e; ensure; end
x.each { |i| puts i }; x.map do |j| j * 2 end; yield 1; block_given?
lambda { |a| a }; -> (b) { b }; proc {}; loop do end
a && b || !c and d or not e; a <=> b; a =~ /x/; a !~ b; a ** 2; a += 1; a ||= 2; a &&= 3
a.b.c; A::B; A::c; obj&.meth; defined?(x); alias new old; undef m
Foo.bar; Foo; Foo::Bar; FOO.bar; Asciidoctor.convert_file 'x', 'doc.adoc'
puts <<~EOS
  heredoc #{x}
EOS
p <<-'RAW'
  raw #{x}
  RAW
# comment TODO
=begin
block comment
=end
__END__
data"#,
        r#"fu	require
--	 
vs	'json'

fu	require_relative
--	 
st	"x"

cf	module
--	 
cn	M
--	; 
cf	class
--	 
cn	C
--	 
op	<
--	 
cn	B
--	; 
fu	include
--	 
dt	Enumerable
--	; 
fu	extend
--	 
dt	Forwardable

--	  
ot	attr_accessor
--	 
wa	:a
--	, 
wa	:b
--	; 
ot	attr_reader
--	 
wa	:c

--	  
cn	CONST
--	 
op	=
--	 
dv	1

--	  
ot	@ivar
--	 
op	=
--	 
dv	2
--	; 
ot	@@cvar
--	 
op	=
--	 
dv	3
--	; 
va	$gvar
--	 
op	=
--	 
dv	4
--	; 
va	$0
--	; 
va	$stdout

--	  
cf	def
--	 initialize(a, b 
op	=
--	 
dv	1
--	, 
op	*
--	args, 
wa	key:
--	 
dv	2
--	, 
op	**
--	opts, 
op	&
--	blk)

--	    
dv	super
--	; 
dv	self
--	; 
dv	nil
--	; 
dv	true
--	; 
dv	false
--	; 
dv	__FILE__
--	; 
dv	__LINE__
--	; __method__

--	  
cf	end

--	  
cf	def
--	 
dv	self
at	.m
--	 
op	=
--	 
dv	1

--	  
cf	def
--	 method?; 
cf	end
--	; 
cf	def
--	 method!; 
cf	end
--	; 
cf	def
--	 
op	==
--	(o); 
cf	end

cf	end
--	; 
cf	end

--	x 
op	=
--	 
kw	[
dv	1
--	, 
fl	2.5
--	, 
fl	1e3
--	, 
bn	0x1F
--	, 
bn	0b101
--	, 
bn	0o17
--	, 
bn	017
--	, 
dv	1_000
--	, 
dv	3
bu	r
--	, 
dv	2
bu	i
--	, 
sc	?
ch	a
--	, 
sc	?\\n
kw	]

--	h 
op	=
--	 
op	{
--	 
wa	a:
--	 
dv	1
--	, 
st	"b"
--	 
op	=>
--	 
dv	2
--	, 
wa	:c
--	 
op	=>
--	 
dv	3
--	, 
wa	d:
--	 
wa	:sym
--	, 
st	"e"
wa	:
--	 
dv	4
--	 
op	}

--	s 
op	=
--	 
st	"dq 
sc	#{
--	x 
op	+
--	 
dv	1
sc	}
st	 
sc	\\n
st	 
sc	\\t
st	"
--	 
op	+
--	 
vs	'sq #{x} 
st	\\'
vs	'
--	 
op	+
--	 
ot	%q(
vs	pct q
ot	)
--	 
op	+
--	 
ot	%Q[
st	pct Q 
sc	#{
--	y
sc	}
ot	]
--	 
op	+
--	 
ot	%w[
vs	a b
ot	]
--	 
op	+
--	 
ot	%i[
vs	c d
ot	]
--	 
op	+
--	 
in	`cmd`

--	r 
op	=
--	 
ss	/re(gex)?
sc	\\d
ss	+/i
--	; r2 
op	=
--	 
ot	%r{
st	a/b
ot	}
ss	x

cf	if
--	 x 
cf	then
--	 y 
cf	elsif
--	 z 
cf	then
--	 w 
cf	else
--	 v 
cf	end
--	; 
cf	unless
--	 a; 
cf	end
--	; 
cf	while
--	 b 
cf	do
--	 
cf	end
--	; 
cf	until
--	 c; 
cf	end

cf	case
--	 x 
cf	when
--	 
dv	1
--	 
cf	then
--	 
wa	:a
--	 
cf	when
--	 
dv	2
op	..
dv	3
--	 
cf	then
--	 
wa	:b
--	 
cf	else
--	 
wa	:c
--	 
cf	end

cf	for
--	 i 
cf	in
--	 
dv	1
op	..
dv	10
--	 
cf	do
--	 
cf	next
--	; 
cf	break
--	; 
cf	redo
--	; 
cf	retry
--	; 
cf	end

cf	begin
--	; 
fu	raise
--	 
st	"e"
--	; 
cf	rescue
--	 
dt	StandardError
--	 
op	=>
--	 e; 
cf	ensure
--	; 
cf	end

--	x
at	.each
--	 
op	{
--	 
op	|
--	i
op	|
--	 
fu	puts
--	 i 
op	}
--	; x
at	.map
--	 
cf	do
--	 
op	|
--	j
op	|
--	 j 
op	*
--	 
dv	2
--	 
cf	end
--	; 
cf	yield
--	 
dv	1
--	; 
fu	block_given?

fu	lambda
--	 
op	{
--	 
op	|
--	a
op	|
--	 a 
op	}
--	; 
op	->
--	 (b) 
op	{
--	 b 
op	}
--	; 
fu	proc
--	 
op	{}
--	; 
fu	loop
--	 
cf	do
--	 
cf	end

--	a 
op	&&
--	 b 
op	||
--	 
op	!
--	c 
cf	and
--	 d 
cf	or
--	 
cf	not
--	 e; a 
op	<=>
--	 b; a 
op	=~
--	 
ss	/x/
--	; a 
op	!~
--	 b; a 
op	**
--	 
dv	2
--	; a 
op	+=
--	 
dv	1
--	; a 
op	||=
--	 
dv	2
--	; a 
op	&&=
--	 
dv	3

--	a
at	.b.c
--	; 
cn	A
op	::
cn	B
--	; 
cn	A
op	::
at	c
--	; obj
op	&
at	.meth
--	; 
cf	defined?
--	(x); 
kw	alias
--	 new old; 
kw	undef
--	 m

dt	Foo
at	.bar
--	; 
dt	Foo
--	; 
dt	Foo
op	::
dt	Bar
--	; 
cn	FOO
at	.bar
--	; 
dt	Asciidoctor
at	.convert_file
--	 
ch	'x'
--	, 
vs	'doc.adoc'

fu	puts
--	 
op	<<~
cn	EOS

--	  heredoc 
co	#{x}

cn	EOS

fu	p
--	 
op	<<-
vs	'RAW'

--	  raw 
co	#{x}

--	  
cn	RAW

co	# comment 
al	TODO

co	=begin

co	block comment

co	=end

cf	__END__

--	data
"#,
    ),
    (
        r#"toml/01-document"#,
        r#"toml"#,
        false,
        r#"# comment
title = "TOML \"Example\" \t"
[owner]
name = 'literal'
dob = 1979-05-27T07:32:00-08:00
date = 1979-05-27
time = 07:32:00
[database]
ports = [ 8000, 8001, 8002 ]
enabled = true
neg = -1_000
hex = 0xDEAD_BEEF
oct = 0o755
bin = 0b1101
flt = 3.1415
exp = 5e+22
inf = inf
nan = -nan
inline = { x = 1, y = "two" }
"quoted key" = 1
dotted.key.name = 2
'single' = 3
[[products]]
name = "Hammer"
[tool.ruff]
multi = """
Roses are red
Violets are blue"""
lit = '''
raw \n text'''
key = "value" # trailing comment
bad = value
[a.b."c d"]
x=1"#,
        r#"co	# comment

dt	title
--	 
op	=
--	 
st	"TOML 
sc	\\"
st	Example
sc	\\"
st	 
sc	\\t
st	"

kw	[owner]

dt	name
--	 
op	=
--	 
st	'
vs	literal
st	'

dt	dob
--	 
op	=
--	 
bn	1979-05-27T07:32:00-08:00

dt	date
--	 
op	=
--	 
bn	1979-05-27

dt	time
--	 
op	=
--	 
bn	07:32:00

kw	[database]

dt	ports
--	 
op	=
--	 
op	[
--	 
dv	8000
op	,
--	 
dv	8001
op	,
--	 
dv	8002
--	 
op	]

dt	enabled
--	 
op	=
--	 
cn	true

dt	neg
--	 
op	=
--	 
dv	-1_000

dt	hex
--	 
op	=
--	 
dv	0xDEAD_BEEF

dt	oct
--	 
op	=
--	 
dv	0o755

dt	bin
--	 
op	=
--	 
dv	0b1101

dt	flt
--	 
op	=
--	 
fl	3.1415

dt	exp
--	 
op	=
--	 
fl	5e+22

dt	inf
--	 
op	=
--	 
fl	inf

dt	nan
--	 
op	=
--	 
fl	-nan

dt	inline
--	 
op	=
--	 
op	{ 
dt	x
op	 =
--	 
dv	1
op	, 
dt	y
op	 =
--	 
st	"two"
op	 }

dt	"quoted key"
--	 
op	=
--	 
dv	1

dt	dotted.key.name
--	 
op	=
--	 
dv	2

dt	'single'
--	 
op	=
--	 
dv	3

kw	[[products]]

dt	name
--	 
op	=
--	 
st	"Hammer"

kw	[tool.ruff]

dt	multi
--	 
op	=
--	 
st	"""

st	Roses are red

st	Violets are blue"""

dt	lit
--	 
op	=
--	 
st	'''

vs	raw \\n text
st	'''

dt	key
--	 
op	=
--	 
st	"value"
--	 
co	# trailing comment

dt	bad
--	 
op	=
--	 
dt	value

kw	[a.b."
dt	c d"
kw	]

dt	x
op	=
dv	1
"#,
    ),
    (
        r#"xml/01-document"#,
        r#"xml"#,
        false,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE root SYSTEM "r.dtd" [ <!ENTITY e "v"> <!ELEMENT a (#PCDATA)> ]>
<!-- comment TODO -->
<root xmlns:x="u" a="1" b='2'>
  <x:elem attr = "v">text &amp; &#169; &#x41; &bad &e;</x:elem>
  <![CDATA[ <raw> & ]]>
  <?pi target data?>
  <empty/>
  <_u.n-d>t</_u.n-d>
  < bad>
  <a attr>x</a>
  <b a="1"c="2"/>
</root>"#,
        r#"fu	<?xml
--	 
ot	version=
st	"1.0"
--	 
ot	encoding=
st	"UTF-8"
fu	?>

dt	<!DOCTYPE
--	 
dt	root
--	 SYSTEM "r.dtd" 
dt	[
--	 
dt	<!ENTITY
--	 
dt	e
--	 
st	"v"
dt	>
--	 
dt	<!ELEMENT
--	 
dt	a
--	 (#PCDATA)
dt	>
--	 
dt	]>

co	<!-- comment 
al	TODO
co	 -->

--	<
kw	root
--	 
ot	xmlns:x=
st	"u"
--	 
ot	a=
st	"1"
--	 
ot	b=
st	'2'
--	>

--	  <
kw	x:elem
--	 
ot	attr
--	 
ot	=
--	 
st	"v"
--	>text 
dv	&amp;
--	 
dv	&#169;
--	 
dv	&#x41;
--	 
er	&
--	bad 
dv	&e;
--	</
kw	x:elem
--	>

--	  
bn	<![CDATA[
--	 <raw> & 
bn	]]>

--	  
fu	<?pi
--	 target data
fu	?>

--	  <
kw	empty
--	/>

--	  <
kw	_u.n-d
--	>t</
kw	_u.n-d
--	>

--	  
er	<
--	 bad>

--	  <
kw	a
--	 
ot	attr
er	>
--	x</
kw	a
--	>

--	  <
kw	b
--	 
ot	a=
st	"1"
ot	c=
st	"2"
--	/>

--	</
kw	root
--	>
"#,
    ),
    (
        r#"yaml/01-document"#,
        r#"yaml"#,
        false,
        r#"%YAML 1.2
---
# comment
key: value
num: 1
float: 1.5
neg: -2
hex: 0x1F
bool: true
nul: null
tilde: ~
str: "double \n quoted"
sq: 'single ''q'''
list:
  - a
  - b: c
    d: e
  - [x, y, "z"]
  - {k: v, k2: 'v2'}
anchor: &anc value
ref: *anc
merge:
  <<: *anc
tagged: !!str 123
custom: !mytag x
multi: |
  line one
  line two
folded: >-
  folded text
key with spaces: v
"quoted key": v
? complex key
: complex value
url: http://x.y/z#frag
time: 2001-12-14t21:59:43.10-05:00
empty:"#,
        r#"pp	%YAML 1.2

pp	---

co	# comment

fu	key
kw	:
at	 value

fu	num
kw	:
at	 
dv	1

fu	float
kw	:
at	 
fl	1.5

fu	neg
kw	:
at	 
dv	-2

fu	hex
kw	:
at	 
dv	0x1F

fu	bool
kw	:
at	 
ch	true

fu	nul
kw	:
at	 
ch	null

fu	tilde
kw	:
at	 
ch	~

fu	str
kw	:
at	 
st	"double 
sc	\\n
st	 quoted"

fu	sq
kw	:
at	 
st	'single 
sc	''
st	q
sc	''
st	'

fu	list
kw	:

at	  
kw	- 
at	a

at	  
kw	- 
fu	b
kw	:
at	 c

at	    
fu	d
kw	:
at	 e

at	  
kw	- [
at	x
kw	,
at	 
ch	y
kw	,
at	 
st	"z"
kw	]

at	  
kw	- {
fu	k
kw	:
fu	 
at	v
kw	,
at	 
fu	k2
kw	:
fu	 
st	'v2'
kw	}

fu	anchor
kw	:
at	 
ot	&anc
at	 value

fu	ref
kw	:
at	 
ot	*anc

fu	merge
kw	:

at	  
fu	<<
kw	:
at	 
ot	*anc

fu	tagged
kw	:
at	 
ot	!!str
at	 
dv	123

fu	custom
kw	:
at	 
ot	!mytag
at	 x

fu	multi
kw	:
at	 
ch	|

at	  line one

at	  line two

fu	folded
kw	:
at	 
ch	>-

at	  folded text

fu	key with spaces
kw	:
at	 v

fu	"quoted key"
kw	:
at	 v

kw	? 
fu	complex key

kw	: 
at	complex value

fu	url
kw	:
at	 http://x.y/z#frag

fu	time
kw	:
at	 
bn	2001-12-14t21:59:43.10-05:00

fu	empty
kw	:
"#,
    ),
    (
        r#"yaml/02-plain-and-flow"#,
        r#"yaml"#,
        false,
        r#"just text here
plain: text with: colon and # not comment
k: v # comment
k2: "s" # c
- item
-item
k3:v3
 indented: x
k4:   spaced
k5: 'a
  b'
k6: [a, [b, c], {d: e}]
k7: {a: [1, 2], b: {c: d}}
k8: yes no on off True FALSE Null NULL ~ .inf
k9: 2020-01-01 2020-1-1"#,
        r#"at	just text here

fu	plain
kw	:
at	 text with
er	: 
at	colon and
co	 # not comment

fu	k
kw	:
at	 v
co	 # comment

fu	k2
kw	:
at	 
st	"s"
at	 
co	# c

kw	- 
at	item

at	-item

at	k3:v3

at	 
fu	indented
kw	:
at	 x

fu	k4
kw	:
at	   spaced

fu	k5
kw	:
at	 
st	'a

st	  b'

fu	k6
kw	:
at	 
kw	[
at	a
kw	,
at	 
kw	[
at	b
kw	,
at	 c
kw	],
at	 
kw	{
fu	d
kw	:
fu	 
at	e
kw	}]

fu	k7
kw	:
at	 
kw	{
fu	a
kw	:
fu	 
kw	[
dv	1
kw	,
at	 
dv	2
kw	],
at	 
fu	b
kw	:
fu	 
kw	{
fu	c
kw	:
fu	 
at	d
kw	}}

fu	k8
kw	:
at	 yes no on off True FALSE Null NULL ~ .inf

fu	k9
kw	:
at	 2020-01-01 2020-1-1
"#,
    ),
    (
        r#"orgmode/embed-yaml"#,
        r#"orgmode"#,
        false,
        r#"#+BEGIN_SRC yaml
key: value
#+END_SRC
after *bold*"#,
        r#"in	#+BEGIN_SRC yaml

fu	key
kw	:
at	 value

co	#+END_SRC

at	after *bold*
"#,
    ),
    (
        r#"orgmode/embed-ruby"#,
        r#"orgmode"#,
        false,
        r#"#+BEGIN_SRC ruby
def f(x) = puts "a#{x}"
#+END_SRC
after *bold*"#,
        r#"in	#+BEGIN_SRC ruby

cf	def
--	 f(x) 
op	=
--	 
fu	puts
--	 
st	"a
sc	#{
--	x
sc	}
st	"

in	#+END_SRC

--	after *bold*
"#,
    ),
    (
        r#"c-cpp C inline: #define"#,
        r#"C"#,
        true,
        r#"#define"#,
        r#"pp	#define
"#,
    ),
    (
        r#"c-cpp C inline: my_align_as(X)"#,
        r#"C"#,
        true,
        r#"my_align_as(X)"#,
        r#"--	my_align_as
op	(
--	X
op	)
"#,
    ),
    (
        r#"c-cpp C inline: ()"#,
        r#"C"#,
        true,
        r#"()"#,
        r#"op	()
"#,
    ),
    (
        r#"c-cpp C inline: []"#,
        r#"C"#,
        true,
        r#"[]"#,
        r#"op	[]
"#,
    ),
    (
        r#"c-cpp C inline: {}"#,
        r#"C"#,
        true,
        r#"{}"#,
        r#"op	{}
"#,
    ),
    (
        r#"c-cpp C++ inline: #define"#,
        r#"C++"#,
        true,
        r#"#define"#,
        r#"pp	#define
"#,
    ),
    (
        r#"c-cpp C++ inline: my_align_as(X)"#,
        r#"C++"#,
        true,
        r#"my_align_as(X)"#,
        r#"--	my_align_as
op	(
--	X
op	)
"#,
    ),
    (
        r#"c-cpp C++ inline: ()"#,
        r#"C++"#,
        true,
        r#"()"#,
        r#"op	()
"#,
    ),
    (
        r#"c-cpp C++ inline: []"#,
        r#"C++"#,
        true,
        r#"[]"#,
        r#"op	[]
"#,
    ),
    (
        r#"c-cpp C++ inline: {}"#,
        r#"C++"#,
        true,
        r#"{}"#,
        r#"op	{}
"#,
    ),
    (
        r#"go/w10-probe"#,
        r#"go"#,
        false,
        r#"// Package main is a doc comment. TODO: fix
/* block
   comment */
package main

import (
	"fmt"
	str "strings"
)

type Point struct {
	X, Y int `json:"x"`
}

type Shape interface{ Area() float64 }

const Pi = 3.14
var big = 1_000_000 + 0x1F + 0o17 + 017 + 0b101 + 1.5e-3 + 2i + 'a' + '\n' + '\x41'

func (p *Point) Move(dx int) (int, error) {
	if p == nil { return 0, nil }
	for i := range 10 { defer fmt.Println(i) }
	switch x := dx; { case x > 0: fallthrough; default: break }
	go func() { select {} }()
	ch := make(chan<- string, 3)
	s := `raw
string` + "esc\t\"q\" %d %v"
	_ = append([]byte(s), str.ToUpper("x")...)
	var b bool = true && !false || iota == 0
	goto end
end:
	panic(len(s) + cap(ch) + copy(nil, nil))
	return int(uint8(rune(dx))), fmt.Errorf("e")
}"#,
        r#"co	// Package main is a doc comment. 
al	TODO
co	: fix

co	/* block

co	   comment */

kw	package
--	 main


kw	import
--	 
op	(

--	\t
st	"fmt"

--	\tstr 
st	"strings"

op	)


kw	type
--	 Point 
kw	struct
--	 
op	{

--	\tX
op	,
--	 Y 
dt	int
--	 
st	`json:"x"`

op	}


kw	type
--	 Shape 
kw	interface
op	{
--	 Area
op	()
--	 
dt	float64
--	 
op	}


kw	const
--	 Pi 
op	=
--	 
fl	3.14

kw	var
--	 big 
op	=
--	 
dv	1_000_000
--	 
op	+
--	 
bn	0x1F
--	 
op	+
--	 
bn	0o17
--	 
op	+
--	 
bn	017
--	 
op	+
--	 
bn	0b101
--	 
op	+
--	 
fl	1.5e-3
--	 
op	+
--	 
dv	2
bu	i
--	 
op	+
--	 
ch	'a'
--	 
op	+
--	 
ch	'\\n'
--	 
op	+
--	 
ch	'\\x41'


kw	func
--	 
op	(
--	p 
op	*
--	Point
op	)
--	 Move
op	(
--	dx 
dt	int
op	)
--	 
op	(
dt	int
op	,
--	 
dt	error
op	)
--	 
op	{

--	\t
cf	if
--	 p 
op	==
--	 
ot	nil
--	 
op	{
--	 
cf	return
--	 
dv	0
op	,
--	 
ot	nil
--	 
op	}

--	\t
cf	for
--	 i 
op	:=
--	 
kw	range
--	 
dv	10
--	 
op	{
--	 
cf	defer
--	 fmt
op	.
--	Println
op	(
--	i
op	)
--	 
op	}

--	\t
cf	switch
--	 x 
op	:=
--	 dx
op	;
--	 
op	{
--	 
cf	case
--	 x 
op	>
--	 
dv	0
op	:
--	 
cf	fallthrough
op	;
--	 
cf	default
op	:
--	 
cf	break
--	 
op	}

--	\t
cf	go
--	 
kw	func
op	()
--	 
op	{
--	 
cf	select
--	 
op	{}
--	 
op	}()

--	\tch 
op	:=
--	 
bu	make
op	(
kw	chan
op	<-
--	 
dt	string
op	,
--	 
dv	3
op	)

--	\ts 
op	:=
--	 
st	`raw

st	string`
--	 
op	+
--	 
st	"esc
ch	\\t\\"
st	q
ch	\\"
st	 %d %v"

--	\t_ 
op	=
--	 
bu	append
op	([]
dt	byte
op	(
--	s
op	),
--	 str
op	.
--	ToUpper
op	(
st	"x"
op	)...)

--	\t
kw	var
--	 b 
dt	bool
--	 
op	=
--	 
ot	true
--	 
op	&&
--	 
op	!
ot	false
--	 
op	||
--	 
ot	iota
--	 
op	==
--	 
dv	0

--	\t
cf	goto
--	 end

--	end
op	:

--	\t
bu	panic
op	(
bu	len
op	(
--	s
op	)
--	 
op	+
--	 
bu	cap
op	(
--	ch
op	)
--	 
op	+
--	 
bu	copy
op	(
ot	nil
op	,
--	 
ot	nil
op	))

--	\t
cf	return
--	 
dt	int
op	(
dt	uint8
op	(
dt	rune
op	(
--	dx
op	))),
--	 fmt
op	.
--	Errorf
op	(
st	"e"
op	)

op	}

"#,
    ),
    (
        r#"lua/w10-probe"#,
        r#"lua"#,
        false,
        r#"-- line comment TODO
--[[ block
comment ]]
--[==[ level comment ]==]
local M = {}
local function add(a, b) return a + b end
function M.new(...)
  local self = setmetatable({}, { __index = M })
  local s = "esc \n \"q\" \x41 \65" .. 'single' .. [[long
string]] .. [==[lvl]==]
  local n = 0x1F + 1.5e3 + 0xA.8p1 + 10 // 3 + 2 ^ 2 % 1
  if n ~= nil and not false or true then
    for i = 1, #s do print(i) end
  elseif type(n) == "number" then
    while true do break end
  else
    repeat goto done until n >= 1
  end
  ::done::
  return self, string.format("%d", n), table.insert, io.write, math.floor(1), os.time()
end
local t <const> = { x = 1, ["k"] = 2; 3 }
return M"#,
        r#"co	-- line comment 
al	TODO

co	--[[ block

co	comment ]]

co	--[==[ level comment ]==]

kw	local
--	 
cn	M
--	 
op	=
--	 
op	{}

kw	local
--	 
kw	function
--	 add
op	(
va	a
op	,
--	 
va	b
op	)
--	 
cf	return
--	 
va	a
--	 
op	+
--	 
va	b
--	 
kw	end

kw	function
--	 
cn	M
op	.
--	new
op	(...)

--	  
kw	local
--	 
va	self
--	 
op	=
--	 
fu	setmetatable
op	({},
--	 
op	{
--	 
bu	__index
--	 
op	=
--	 
cn	M
--	 
op	})

--	  
kw	local
--	 
va	s
--	 
op	=
--	 
st	"esc 
sc	\\n
st	 
sc	\\"
st	q
sc	\\"
st	 
sc	\\x41
st	 
sc	\\65
st	"
--	 
op	..
--	 
st	'single'
--	 
op	..
--	 
vs	[[long

vs	string]]
--	 
op	..
--	 
vs	[==[lvl]==]

--	  
kw	local
--	 
va	n
--	 
op	=
--	 
dv	0x1F
--	 
op	+
--	 
dv	1.5e3
--	 
op	+
--	 
dv	0xA.8p1
--	 
op	+
--	 
dv	10
--	 
op	//
--	 
dv	3
--	 
op	+
--	 
dv	2
--	 
op	^
--	 
dv	2
--	 
op	%
--	 
dv	1

--	  
cf	if
--	 
va	n
--	 
op	~=
--	 
kw	nil
--	 
kw	and
--	 
kw	not
--	 
kw	false
--	 
kw	or
--	 
kw	true
--	 
cf	then

--	    
cf	for
--	 
va	i
--	 
op	=
--	 
dv	1
op	,
--	 
op	#
va	s
--	 
cf	do
--	 
fu	print
op	(
va	i
op	)
--	 
cf	end

--	  
cf	elseif
--	 
fu	type
op	(
va	n
op	)
--	 
op	==
--	 
st	"number"
--	 
cf	then

--	    
cf	while
--	 
kw	true
--	 
cf	do
--	 
cf	break
--	 
cf	end

--	  
cf	else

--	    
cf	repeat
--	 
cf	goto
--	 
va	done
--	 
cf	until
--	 
va	n
--	 
op	>=
--	 
dv	1

--	  
cf	end

--	  
op	::
va	done
op	::

--	  
cf	return
--	 
va	self
op	,
--	 
fu	string.format
op	(
st	"%d"
op	,
--	 
va	n
op	),
--	 
fu	table.insert
op	,
--	 
fu	io.write
op	,
--	 
fu	math.floor
op	(
dv	1
op	),
--	 
fu	os.time
op	()

kw	end

kw	local
--	 
va	t
--	 
at	<const>
--	 
op	=
--	 
op	{
--	 
va	x
--	 
op	=
--	 
dv	1
op	,
--	 
op	[
st	"k"
op	]
--	 
op	=
--	 
dv	2
op	;
--	 
dv	3
--	 
op	}

cf	return
--	 
cn	M

"#,
    ),
    (
        r#"perl/w10-probe"#,
        r#"perl"#,
        false,
        r#"#!/usr/bin/perl
# line comment TODO
use strict;
use warnings;
use List::Util qw(sum max);
package Foo::Bar;
our $VERSION = '1.0';
my ($x, @arr, %h) = (1, (1, 2), (a => 1));
my $s = "interp $x @arr $h{a} ${x} \n\t";
my $t = 'single \' q';
my $n = 0x1F + 0b101 + 017 + 1_000 + 1.5e3;
if ($s =~ m/^a(b+)c$/i && $t !~ /x/) { print "match $1\n"; }
$s =~ s/foo/bar/g;
$s =~ tr/a-z/A-Z/;
my @w = qw(a b c);
my $q = qq{braced $x}; my $r = q(paren);
my $re = qr/\d+\s*/x;
sub greet { my ($name) = @_; return "Hi $name"; }
foreach my $i (0..$#arr) { next if $i; last; }
unless (defined $x) { die "no"; } else { warn 'w'; }
open(my $fh, '<', $file) or die "cannot: $!";
print STDERR <<"EOT";
heredoc $x
EOT
print <<'RAW';
raw $x
RAW
local $_ = shift; $x->{key}[0]->method(@ARGV);
__END__
trailing pod"#,
        r#"kw	#!/usr/bin/perl

co	# line comment 
al	TODO

fu	use
--	 
kw	strict
--	;

fu	use
--	 
kw	warnings
--	;

fu	use
--	 
fu	List::Util
--	 
fu	qw
--	(sum max);

kw	package
--	 
fu	Foo::Bar
--	;

kw	our
--	 
dt	$VERSION
--	 = 
ot	'
ss	1.0
ot	'
--	;

kw	my
--	 (
dt	$x
--	, 
dt	@arr
--	, 
dt	%h
--	) = (
dv	1
--	, (
dv	1
--	, 
dv	2
--	), (a => 
dv	1
--	));

kw	my
--	 
dt	$s
--	 = 
ot	"
st	interp 
dt	$x
st	 
dt	@arr
st	 
dt	$h
st	{a} 
dt	${x}
st	 
ch	\\n\\t
ot	"
--	;

kw	my
--	 
dt	$t
--	 = 
ot	'
ss	single 
ch	\\'
ss	 q
ot	'
--	;

kw	my
--	 
dt	$n
--	 = 
bn	0x1F
--	 + 
bn	0b101
--	 + 
bn	017
--	 + 
dv	1_000
--	 + 
fl	1.5e3
--	;

cf	if
--	 (
dt	$s
--	 =~ 
ot	m/
ch	^
ot	a
ch	(
ot	b
ch	+)
ot	c
ch	$
ot	/i
--	 && 
dt	$t
--	 !~ 
ot	/x/
--	) { 
fu	print
--	 
ot	"
st	match 
wa	$1
ch	\\n
ot	"
--	; }

dt	$s
--	 =~ 
ot	s/foo/
st	bar
ot	/g
--	;

dt	$s
--	 =~ 
ot	tr/a-z/A-Z/
--	;

kw	my
--	 
dt	@w
--	 = 
ot	qw(
--	a b c
ot	)
--	;

kw	my
--	 
dt	$q
--	 = 
ot	qq{
st	braced 
dt	$x
ot	}
--	; 
kw	my
--	 
dt	$r
--	 = 
ot	q(
ss	paren
ot	)
--	;

kw	my
--	 
dt	$re
--	 = 
ot	qr/
bn	\\d
ch	+
bn	\\s
ch	*
ot	/x
--	;

kw	sub 
fu	greet
--	 { 
kw	my
--	 (
dt	$name
--	) = 
dt	@_
--	; 
cf	return
--	 
ot	"
st	Hi 
dt	$name
ot	"
--	; }

cf	foreach
--	 
kw	my
--	 
dt	$i
--	 (
fl	0.
--	.
dt	$#arr
--	) { 
cf	next
--	 
cf	if
--	 
dt	$i
--	; 
cf	last
--	; }

cf	unless
--	 (
fu	defined
--	 
dt	$x
--	) { 
fu	die
--	 
ot	"
st	no
ot	"
--	; } 
cf	else
--	 { 
fu	warn
--	 
ot	'
ss	w
ot	'
--	; }

fu	open
--	(
kw	my
--	 
dt	$fh
--	, 
ot	'
ss	<
ot	'
--	, 
dt	$file
--	) 
ot	or
--	 
fu	die
--	 
ot	"
st	cannot: 
wa	$!
ot	"
--	;

fu	print
--	 STDERR 
ot	<<
kw	"EOT";

st	heredoc 
dt	$x

kw	EOT

fu	print
--	 
ot	<<
kw	'RAW';

--	raw $x

kw	RAW

kw	local
--	 
wa	$_
--	 = 
fu	shift
--	; 
dt	$x
--	->{key}[
dv	0
--	]->method(
wa	@ARGV
--	);

kw	__END__

--	trailing pod

"#,
    ),
    (
        r#"cs/w10-probe"#,
        r#"cs"#,
        false,
        r#"// line comment TODO
/* block */
/// <summary>Doc comment</summary>
#region Setup
#if DEBUG
using System;
using System.Collections.Generic;
#endif
namespace App.Core;
[Serializable, Obsolete("x")]
public sealed class Foo<T> : Base, IDisposable where T : class
{
    private readonly int _n = 0x1F + 0b1010 + 1_000 + 10L + 2.5f + 3.0m + 1e3d;
    public string Name { get; init; } = @"verbatim ""q"" \n";
    public event EventHandler? Changed;
    public async Task<int> RunAsync(object o, ref int r, out bool b)
    {
        var s = $"interp {Name} {_n:X} \t";
        var raw = """
            raw "string"
            """;
        char c = 'c'; char e = '\n';
        if (o is null) throw new ArgumentNullException(nameof(o));
        foreach (var x in new List<int> { 1, 2 }) { yield return x; }
        switch (r) { case 1: break; default: goto end; }
        b = true && !false;
        lock (this) { checked { r++; } }
        using var d = new MemoryStream();
        end: return await Task.FromResult(sizeof(int) + typeof(T).Name.Length);
    }
    record Person(string First, int Age);
    dynamic dyn = null; decimal m; ulong u; nint ni;
}
#endregion"#,
        r#"co	// line comment 
al	TODO

co	/* block */

co	/// 
kw	<summary>
co	Doc comment
kw	</summary>

dv	#region Setup

kw	#if
--	 DEBUG

kw	using
--	 System
op	;

kw	using
--	 System
op	.
fu	Collections
op	.
fu	Generic
op	;

kw	#endif

kw	namespace
--	 App
op	.
fu	Core
op	;

op	[
--	Serializable
op	,
--	 
fu	Obsolete
op	(
st	"x"
op	)]

kw	public
--	 
kw	sealed
--	 
kw	class
--	 Foo
op	<
--	T
op	>
--	 
op	:
--	 Base
op	,
--	 IDisposable where T 
op	:
--	 
kw	class

op	{

--	    
kw	private
--	 
kw	readonly
--	 
dt	int
--	 _n 
op	=
--	 
bn	0x1F
--	 
op	+
--	 
dv	0
--	b1010 
op	+
--	 
dv	1
--	_000 
op	+
--	 
dv	10L
--	 
op	+
--	 
fl	2.5f
--	 
op	+
--	 
fl	3.0m
--	 
op	+
--	 
dv	1
--	e3d
op	;

--	    
kw	public
--	 
dt	string
--	 Name 
op	{
--	 
kw	get
op	;
--	 init
op	;
--	 
op	}
--	 
op	=
--	 @
st	"verbatim ""q"" 
sc	\\n
st	"
op	;

--	    
kw	public
--	 
kw	event
--	 EventHandler
op	?
--	 Changed
op	;

--	    
kw	public
--	 async Task
op	<
dt	int
op	>
--	 
fu	RunAsync
op	(
dt	object
--	 o
op	,
--	 
kw	ref
--	 
dt	int
--	 r
op	,
--	 
kw	out
--	 
dt	bool
--	 b
op	)

--	    
op	{

--	        
dt	var
--	 s 
op	=
--	 $
st	"interp {Name} {_n:X} 
sc	\\t
st	"
op	;

--	        
dt	var
--	 raw 
op	=
--	 
st	"""

--	            raw 
st	"string"

--	            
st	""";

--	        
dt	char
--	 c 
op	=
--	 
ch	'c'
op	;
--	 
dt	char
--	 e 
op	=
--	 
ch	'\\n'
op	;

--	        
kw	if
--	 
op	(
--	o 
kw	is
--	 
kw	null
op	)
--	 
kw	throw
--	 
kw	new
--	 
fu	ArgumentNullException
op	(
fu	nameof
op	(
--	o
op	));

--	        
kw	foreach
--	 
op	(
dt	var
--	 x 
kw	in
--	 
kw	new
--	 List
op	<
dt	int
op	>
--	 
op	{
--	 
dv	1
op	,
--	 
dv	2
--	 
op	})
--	 
op	{
--	 
kw	yield
--	 
kw	return
--	 x
op	;
--	 
op	}

--	        
kw	switch
--	 
op	(
--	r
op	)
--	 
op	{
--	 
kw	case
--	 
dv	1
op	:
--	 
kw	break
op	;
--	 
kw	default
op	:
--	 
kw	goto
--	 end
op	;
--	 
op	}

--	        b 
op	=
--	 
kw	true
--	 
op	&&
--	 
op	!
kw	false
op	;

--	        
kw	lock
--	 
op	(
kw	this
op	)
--	 
op	{
--	 
kw	checked
--	 
op	{
--	 r
op	++;
--	 
op	}
--	 
op	}

--	        
kw	using
--	 
dt	var
--	 d 
op	=
--	 
kw	new
--	 
fu	MemoryStream
op	();

--	        end
op	:
--	 
kw	return
--	 await Task
op	.
fu	FromResult
op	(
kw	sizeof
op	(
dt	int
op	)
--	 
op	+
--	 
kw	typeof
op	(
--	T
op	).
fu	Name
op	.
fu	Length
op	);

--	    
op	}

--	    record 
fu	Person
op	(
dt	string
--	 First
op	,
--	 
dt	int
--	 Age
op	);

--	    dynamic dyn 
op	=
--	 
kw	null
op	;
--	 
dt	decimal
--	 m
op	;
--	 
dt	ulong
--	 u
op	;
--	 nint ni
op	;

op	}

dv	#endregion

"#,
    ),
    (
        r#"php/w10-probe"#,
        r#"php"#,
        false,
        r#"<!DOCTYPE html>
<p class="x">Hello <?= $name ?></p>
<?php
// line comment TODO
# hash comment
/* block */
/** @param int $x Doc */
declare(strict_types=1);
namespace App\Models;
use Foo\Bar as Baz;
#[Attribute]
final class User extends Model implements JsonSerializable {
    public const MAX = 0x1F + 0b11 + 017 + 1_000 + 1.5e3;
    private ?string $name = null;
    public function __construct(private readonly int $id = 0) {}
    public static function find(array $ids, callable $cb): static|false {
        $s = "interp $name {$this->id} ${x} \n";
        $t = 'single \' q';
        $h = <<<EOT
heredoc $name
EOT;
        $n = <<<'NOW'
nowdoc
NOW;
        if (isset($ids[0]) && !empty($ids)) { return new static(); }
        foreach ($ids as $k => $v) { echo $v; continue; }
        match(true) { default => throw new \Exception("e") };
        $fn = fn($x) => $x * 2;
        return array_map($cb, $ids) ?? TRUE;
    }
}
?>
<div>after</div>"#,
        r#"op	<!
cn	DOCTYPE
--	 html
op	>

op	<
--	p 
kw	class
op	=
st	"x"
op	>
--	Hello 
kw	<?
op	=
--	 
va	$name
--	 
kw	?>
op	</
--	p
op	>

kw	<?php

co	// line comment 
al	TODO

co	# hash comment

co	/* block */

co	/** 
an	@param
co	 
cv	int
co	 $x Doc */

kw	declare
--	(strict_types
op	=
dv	1
--	)
ot	;

kw	namespace
--	 App\\Models
ot	;

kw	use
--	 Foo\\Bar 
kw	as
--	 Baz
ot	;

at	#[
--	Attribute
at	]

kw	final
--	 
kw	class
--	 User 
kw	extends
--	 Model 
kw	implements
--	 
bu	JsonSerializable
--	 {

--	    
kw	public
--	 
kw	const
--	 
fu	MAX
--	 
op	=
--	 
bn	0x1F
--	 
op	+
--	 
bn	0b11
--	 
op	+
--	 
bn	017
--	 
op	+
--	 
dv	1_000
--	 
op	+
--	 
fl	1.5e3
ot	;

--	    
kw	private
--	 
ot	?
dt	string
--	 
va	$name
--	 
op	=
--	 
kw	null
ot	;

--	    
kw	public
--	 
kw	function
--	 
bu	__construct
--	(
kw	private
--	 
dt	readonly
--	 
dt	int
--	 
va	$id
--	 
op	=
--	 
dv	0
--	) {}

--	    
kw	public
--	 
kw	static
--	 
kw	function
--	 find(
dt	array
--	 
va	$ids
ot	,
--	 
kw	callable
--	 
va	$cb
--	)
ot	:
--	 
kw	static
op	|
kw	false
--	 {

--	        
va	$s
--	 
op	=
--	 
st	"interp 
va	$name
st	 
--	{
va	$this
--	->id}
st	 
va	$
--	{
va	x
--	}
st	 
sc	\\n
st	"
ot	;

--	        
va	$t
--	 
op	=
--	 
st	'single 
sc	\\'
st	 q'
ot	;

--	        
va	$h
--	 
op	=
--	 
ss	<<<EOT

st	heredoc 
va	$name

ss	EOT
--	;

--	        
va	$n
--	 
op	=
--	 
vs	<<<'NOW'

st	nowdoc

vs	NOW
--	;

--	        
cf	if
--	 (
kw	isset
--	(
va	$ids
--	[
dv	0
--	]) 
op	&&
--	 
op	!
kw	empty
--	(
va	$ids
--	)) { 
cf	return
--	 
kw	new
--	 
kw	static
--	()
ot	;
--	 }

--	        
cf	foreach
--	 (
va	$ids
--	 
kw	as
--	 
va	$k
--	 => 
va	$v
--	) { 
kw	echo
--	 
va	$v
ot	;
--	 
cf	continue
ot	;
--	 }

--	        
cf	match
--	(
kw	true
--	) { 
cf	default
--	 => 
cf	throw
--	 
kw	new
--	 \\
bu	Exception
--	(
st	"e"
--	) }
ot	;

--	        
va	$fn
--	 
op	=
--	 
kw	fn
--	(
va	$x
--	) => 
va	$x
--	 
op	*
--	 
dv	2
ot	;

--	        
cf	return
--	 
fu	array_map
--	(
va	$cb
ot	,
--	 
va	$ids
--	) 
op	??
--	 
kw	TRUE
ot	;

--	    }

--	}

kw	?>

op	<
--	div
op	>
--	after
op	</
--	div
op	>

"#,
    ),
    (
        r#"sql/w10-probe"#,
        r#"sql"#,
        false,
        r#"-- line comment TODO
/* block
   comment */
SELECT a.id, COUNT(*) AS n, MAX(b.x), 'it''s', "Quoted Id", `bt`
FROM users a LEFT OUTER JOIN orders b ON a.id = b.uid
WHERE a.age >= 18 AND b.total <> 0 OR NOT a.flag IS NULL
GROUP BY a.id HAVING n > 1 ORDER BY n DESC LIMIT 10;
INSERT INTO t (c1, c2) VALUES (1, 2.5e3), (0x1F, -3);
UPDATE t SET c = TRUE WHERE id IN (1, 2) ;
CREATE TABLE IF NOT EXISTS t (id INTEGER PRIMARY KEY, name VARCHAR(20) NOT NULL, d DATE, ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP);
select lower(name), coalesce(x, 0), cast(y as numeric) from t where x between 1 and 2;
BEGIN TRANSACTION; COMMIT; ROLLBACK;
DROP INDEX idx; # hash comment
CASE WHEN x = 1 THEN 'a' ELSE 'b' END
x || y, @var, :param, ?"#,
        r#"co	-- line comment 
al	TODO

co	/* block

co	   comment */

kw	SELECT
--	 a.
kw	id
--	, 
fu	COUNT
--	(
op	*
--	) 
kw	AS
--	 n, 
fu	MAX
--	(b.x), 
st	'it
ch	''
st	s'
--	, 
ot	"Quoted Id"
--	, `bt`

kw	FROM
--	 users a 
kw	LEFT
--	 
kw	OUTER
--	 
kw	JOIN
--	 orders b 
kw	ON
--	 a.
kw	id
--	 
op	=
--	 b.
fu	uid

kw	WHERE
--	 a.age 
op	>=
--	 
dv	18
--	 
kw	AND
--	 b.total 
op	<>
--	 
dv	0
--	 
kw	OR
--	 
kw	NOT
--	 a.flag 
kw	IS
--	 
kw	NULL

kw	GROUP
--	 
kw	BY
--	 a.
kw	id
--	 
kw	HAVING
--	 n 
op	>
--	 
dv	1
--	 
kw	ORDER
--	 
kw	BY
--	 n 
kw	DESC
--	 
kw	LIMIT
--	 
dv	10
--	;

kw	INSERT
--	 
kw	INTO
--	 t (c1, c2) 
kw	VALUES
--	 (
dv	1
--	, 
fl	2.5e3
--	), (
bn	0x1F
--	, 
op	-
dv	3
--	);

kw	UPDATE
--	 t 
kw	SET
--	 c 
op	=
--	 
kw	TRUE
--	 
kw	WHERE
--	 
kw	id
--	 
kw	IN
--	 (
dv	1
--	, 
dv	2
--	) ;

kw	CREATE
--	 
kw	TABLE
--	 
cf	IF
--	 
kw	NOT
--	 
kw	EXISTS
--	 t (
kw	id
--	 
dt	INTEGER
--	 
kw	PRIMARY
--	 
kw	KEY
--	, name 
dt	VARCHAR
--	(
dv	20
--	) 
kw	NOT
--	 
kw	NULL
--	, d 
dt	DATE
--	, ts 
dt	TIMESTAMP
--	 
kw	DEFAULT
--	 
fu	CURRENT_TIMESTAMP
--	);

kw	select
--	 
fu	lower
--	(name), 
fu	coalesce
--	(x, 
dv	0
--	), 
fu	cast
--	(y 
kw	as
--	 
dt	numeric
--	) 
kw	from
--	 t 
kw	where
--	 x 
kw	between
--	 
dv	1
--	 
kw	and
--	 
dv	2
--	;

cf	BEGIN
--	 
kw	TRANSACTION
--	; 
kw	COMMIT
--	; 
kw	ROLLBACK
--	;

kw	DROP
--	 
kw	INDEX
--	 idx; # 
kw	hash
--	 
kw	comment

cf	CASE
--	 
cf	WHEN
--	 x 
op	=
--	 
dv	1
--	 
cf	THEN
--	 
st	'a'
--	 
cf	ELSE
--	 
st	'b'
--	 
cf	END

--	x 
op	||
--	 y, @var, 
ch	:param
--	, ?

"#,
    ),
    (
        r#"sqlmysql/w10-probe"#,
        r#"sqlmysql"#,
        false,
        r#"-- c
# hash comment
/* b */
SELECT `col`, "dq", 'sq\'s', @v, @@global.x FROM t WHERE a <=> b AND x REGEXP '^a';
CREATE TABLE t (id INT UNSIGNED AUTO_INCREMENT, j JSON, e ENUM('a','b')) ENGINE=InnoDB;
DELIMITER //
select now(), concat('a', 'b'), 0x1F, b'101', 1.5e3, NULL, TRUE;"#,
        r#"co	-- c

co	# hash comment

co	/* b */

kw	SELECT
--	 
st	`col`
--	, 
st	"dq"
--	, 
st	'sq
ch	\\'
st	s'
--	, @v, @@global
ch	.
--	x 
kw	FROM
--	 t 
kw	WHERE
--	 a 
op	<=>
--	 b 
kw	AND
--	 x 
kw	REGEXP
--	 
st	'^a'
--	;

kw	CREATE
--	 
kw	TABLE
--	 t (id 
dt	INT
--	 
kw	UNSIGNED
--	 
kw	AUTO_INCREMENT
--	, j JSON, e 
dt	ENUM
--	(
st	'a'
--	,
st	'b'
--	)) ENGINE
op	=
kw	InnoDB
--	;

--	DELIMITER 
op	//

kw	select
--	 
fu	now
--	(), 
fu	concat
--	(
st	'a'
--	, 
st	'b'
--	), 
bn	0x1F
--	, b
st	'101'
--	, 
fl	1.5e3
--	, 
kw	NULL
--	, TRUE;

"#,
    ),
    (
        r#"sqlpostgresql/w10-probe"#,
        r#"sqlpostgresql"#,
        false,
        r#"-- c
/* b */
SELECT $1, 'str', E'esc\n', $$dollar
body$$, $tag$x$tag$, x::int, ARRAY[1,2], 'a' || 'b' FROM t WHERE j @> '{}' AND name ILIKE 'a%';
CREATE OR REPLACE FUNCTION f(a integer) RETURNS SETOF text LANGUAGE plpgsql AS $$ BEGIN RETURN; END $$;
select now(), coalesce(a, 0), 1.5e3, NULL, true from t;"#,
        r#"co	-- c

co	/* b */

kw	SELECT
--	 
op	$1, 'str', E'esc\\n', $
vs	$dollar

vs	body$$, $tag$x$tag$, x::int, ARRAY[1,2], 'a' || 'b' FROM t WHERE j @> '{}' AND name ILIKE 'a%';

vs	CREATE OR REPLACE FUNCTION f(a integer) RETURNS SETOF text LANGUAGE plpgsql AS $$ BEGIN RETURN; END $$;

vs	select now(), coalesce(a, 0), 1.5e3, NULL, true from t;

"#,
    ),
    (
        r#"sqlpostgresql/w10-probe-2"#,
        r#"sqlpostgresql"#,
        false,
        r#"SELECT 'str', E'esc\n', x::int, ARRAY[1,2], 'a' || 'b' FROM t WHERE j @> '{}' AND name ILIKE 'a%';
select now(), coalesce(a, 0), 1.5e3, NULL, true, "QuotedId", $1 from t; -- c
CREATE FUNCTION f() RETURNS int AS $$ SELECT 1 $$ LANGUAGE sql;
DO $body$ BEGIN RAISE NOTICE 'x'; END $body$;"#,
        r#"kw	SELECT
--	 
vs	'str
st	'
--	, 
st	E'esc
ch	\\n
st	'
--	, x
ch	::
dt	int
--	, 
kw	ARRAY
--	[
dv	1
--	,
dv	2
--	], 
vs	'a
st	'
--	 
op	||
--	 
vs	'b
st	'
--	 
kw	FROM
--	 
kw	t
--	 
kw	WHERE
--	 j 
op	@>
--	 
vs	'{}
st	'
--	 
kw	AND
--	 
kw	name
--	 
kw	ILIKE
--	 
vs	'a%
st	'
--	;

kw	select
--	 
fu	now
--	(), 
fu	coalesce
--	(
kw	a
--	, 
dv	0
--	), 
fl	1.5e3
--	, 
kw	NULL
--	, 
kw	true
--	, 
ot	"QuotedId"
--	, $1 
kw	from
--	 
kw	t
--	; 
co	-- c

kw	CREATE FUNCTION
--	 f() 
kw	RETURNS
--	 
dt	int
--	 
kw	AS
--	 
fu	$$
--	 
kw	SELECT
--	 
dv	1
--	 
fu	$$
--	 
kw	LANGUAGE
--	 
kw	sql
--	;

kw	DO $body$
--	 
cf	BEGIN
--	 RAISE NOTICE 
vs	'x
st	'
--	; 
cf	END
--	 
fu	$body$
--	;

"#,
    ),
    (
        r#"cobol/w10-dep116"#,
        r#"cobol"#,
        false,
        r#"           EXEC SQL
               SELECT NAME INTO :WS-NAME FROM USERS WHERE ID = 42
           END-EXEC."#,
        r#"--	           
re	EXEC SQL

--	               
kw	SELECT
--	 NAME 
kw	INTO
--	 
ch	:WS
op	-
--	NAME 
kw	FROM
--	 USERS 
kw	WHERE
--	 
kw	ID
--	 
op	=
--	 
dv	42

--	           
re	END-EXEC
--	.

"#,
    ),
    (
        r#"sas/w10-dep1087"#,
        r#"sas"#,
        false,
        r#"proc sql;
  create table big as
  select name, age from sashelp.class
  where age > 13 order by age;
quit;"#,
        r#"kw	proc sql
--	;

--	  
kw	create
--	 
kw	table
--	 big 
kw	as

--	  
kw	select
--	 name, age 
kw	from
--	 sashelp.
kw	class

--	  
kw	where
--	 age 
op	>
--	 
dv	13
--	 
kw	order
--	 
kw	by
--	 age;

kw	quit;

"#,
    ),
    (
        r#"sas/w10-dep1089"#,
        r#"sas"#,
        false,
        r#"proc lua;
submit;
local x = 10
print("lua " .. x)
endsubmit;
run;"#,
        r#"kw	proc lua
--	;

kw	submit;

kw	local
--	 
va	x
--	 
op	=
--	 
dv	10

fu	print
op	(
st	"lua "
--	 
op	..
--	 
va	x
op	)

kw	endsubmit;

kw	run;

"#,
    ),
    (
        r#"raku/w10-dep1032"#,
        r#"raku"#,
        false,
        r#"my token word { \w+ }
my regex pair { <word> '=' <word> }
say "a=b" ~~ /<pair>/;
say m:P5/^\w+$/;"#,
        r#"kw	my
--	 
kw	token
--	 word {
ot	 
bn	\\w
cf	+
ot	 
--	}

kw	my
--	 
kw	regex
--	 pair {
ot	 
ch	<
bn	word
ch	>
ot	 
ch	'
st	=
ch	'
ot	 
ch	<
bn	word
ch	>
ot	 
--	}

fu	say
--	 
ch	"
st	a=b
ch	"
--	 
op	~~
--	 
vs	/
ch	<
bn	pair
ch	>
vs	/
--	;

fu	say
--	 
vs	m
kw	:P5
ot	/
ch	^
bn	\\w
ch	+$
ot	/
--	;

"#,
    ),
    (
        r#"orgmode/w10-dep911"#,
        r#"orgmode"#,
        false,
        r#"#+BEGIN_SRC rust
fn main() { println!("x"); }
#+END_SRC
#+BEGIN_SRC go
func main() {}
#+END_SRC"#,
        r#"in	#+BEGIN_SRC rust

kw	fn
--	 main() 
op	{
--	 
pp	println!
--	(
st	"x"
--	)
op	;
--	 
op	}

in	#+END_SRC

in	#+BEGIN_SRC go

kw	func
--	 main
op	()
--	 
op	{}

in	#+END_SRC

"#,
    ),
    (
        r#"orgmode/w10-dep912"#,
        r#"orgmode"#,
        false,
        r#"#+BEGIN_SRC html
<p class="x">hi</p>
#+END_SRC
#+BEGIN_SRC css
p { color: red; }
#+END_SRC"#,
        r#"in	#+BEGIN_SRC h
--	tml

op	<
--	p class
op	=
st	"x"
op	>
--	hi
op	</
--	p
op	>

in	#+END_SRC

in	#+BEGIN_SRC cs
--	s

--	p 
op	{
--	 color
op	:
--	 red
op	;
--	 
op	}

in	#+END_SRC

"#,
    ),
    (
        r#"orgmode/w10-dep913"#,
        r#"orgmode"#,
        false,
        r#"#+BEGIN_SRC sql
SELECT * FROM t WHERE id = 1;
#+END_SRC
#+BEGIN_SRC nim
echo "nim"
#+END_SRC
#+BEGIN_SRC odin
x := 1
#+END_SRC"#,
        r#"in	#+BEGIN_SRC sql

kw	SELECT
--	 
op	*
--	 
kw	FROM
--	 t 
kw	WHERE
--	 id 
op	=
--	 
dv	1
--	;

in	#+END_SRC

in	#+BEGIN_SRC nim

cf	echo
--	 
st	"nim"

in	#+END_SRC

in	#+BEGIN_SRC odin

va	x 
op	:=
--	 
dv	1

in	#+END_SRC

"#,
    ),
    (
        r#"orgmode/w10-dep922"#,
        r#"orgmode"#,
        false,
        r#"#+BEGIN_SRC ruby
puts "r"
#+END_SRC
#+BEGIN_SRC java
class A {}
#+END_SRC
#+BEGIN_SRC php
<?php echo 1; ?>
#+END_SRC"#,
        r#"in	#+BEGIN_SRC ruby

fu	puts
--	 
st	"r"

in	#+END_SRC

in	#+BEGIN_SRC java

kw	class
--	 A 
op	{}

in	#+END_SRC

in	#+BEGIN_SRC php

op	<
ot	?
--	php 
kw	echo
--	 
dv	1
ot	;
--	 ?>

--	#+END_SRC

"#,
    ),
    (
        r#"cpp/w10-probe"#,
        r#"cpp"#,
        false,
        r#"// comment
#include <vector>
template <typename T> concept C = requires(T t) { t.f(); };
int main() { auto x = 1'000 + 0x1F + 1.5e3f + 10ull; std::vector<int> v{1}; co_return; constexpr bool b = true; nullptr; R"(raw)"; u8"s"; }"#,
        r#"co	// comment

pp	#include 
im	<vector>

kw	template
--	 
op	<
kw	typename
--	 T
op	>
--	 
kw	concept
--	 C 
op	=
--	 
kw	requires
op	(
--	T t
op	)
--	 
op	{
--	 t
op	.
--	f
op	();
--	 
op	};

dt	int
--	 main
op	()
--	 
op	{
--	 
kw	auto
--	 x 
op	=
--	 
dv	1'000
--	 
op	+
--	 
bn	0x1F
--	 
op	+
--	 
fl	1.5e3
bu	f
--	 
op	+
--	 
dv	10
bu	ull
op	;
--	 
bu	std::
--	vector
op	<
dt	int
op	>
--	 v
op	{
dv	1
op	};
--	 
cf	co_return
op	;
--	 
kw	constexpr
--	 
dt	bool
--	 b 
op	=
--	 
kw	true
op	;
--	 
kw	nullptr
op	;
--	 
st	R"(
vs	raw
st	)"
op	;
--	 
st	u8"s"
op	;
--	 
op	}

"#,
    ),
    (
        r#"isocpp/w10-probe"#,
        r#"isocpp"#,
        false,
        r#"// comment
#include <vector>
template <typename T> concept C = requires(T t) { t.f(); };
int main() { auto x = 1'000 + 0x1F + 1.5e3f + 10ull; std::vector<int> v{1}; co_return; constexpr bool b = true; nullptr; R"(raw)"; u8"s"; }"#,
        r#"co	// comment

pp	#include 
im	<vector>

kw	template
--	 
op	<
kw	typename
--	 T
op	>
--	 
kw	concept
--	 C 
op	=
--	 
kw	requires
op	(
--	T t
op	)
--	 
op	{
--	 t
op	.
--	f
op	();
--	 
op	};

dt	int
--	 main
op	()
--	 
op	{
--	 
kw	auto
--	 x 
op	=
--	 
dv	1'000
--	 
op	+
--	 
bn	0x1F
--	 
op	+
--	 
fl	1.5e3
bu	f
--	 
op	+
--	 
dv	10
bu	ull
op	;
--	 
bu	std::
--	vector
op	<
dt	int
op	>
--	 v
op	{
dv	1
op	};
--	 
cf	co_return
op	;
--	 
kw	constexpr
--	 
dt	bool
--	 b 
op	=
--	 
kw	true
op	;
--	 
kw	nullptr
op	;
--	 
st	R"(
vs	raw
st	)"
op	;
--	 
st	u8"s"
op	;
--	 
op	}

"#,
    ),
    (
        r#"w10-fix/yaml flow keys"#,
        r#"yaml"#,
        false,
        r#"{"a": 1}
{'a' : 1, "b\"c": ["d": 2]}
k: {"a": 1, 'b': 2}"#,
        r#"kw	{
fu	"a"
kw	:
fu	 
dv	1
kw	}

kw	{
fu	'a' 
kw	:
fu	 
dv	1
kw	,
at	 
fu	"b
sc	\\"
fu	c"
kw	:
fu	 
kw	[
fu	"d"
kw	:
fu	 
dv	2
kw	]}

fu	k
kw	:
at	 
kw	{
fu	"a"
kw	:
fu	 
dv	1
kw	,
at	 
fu	'b'
kw	:
fu	 
dv	2
kw	}

"#,
    ),
    (
        r#"w10-fix/orgmode yaml stack"#,
        r#"orgmode"#,
        false,
        r#"#+BEGIN_SRC yaml
key: value
list: [1, 2]
#+END_SRC
#+BEGIN_SRC json
{"a": [1, true, null]}
#+END_SRC"#,
        r#"in	#+BEGIN_SRC yaml

fu	key
kw	:
at	 value

fu	list
kw	:
at	 
kw	[
dv	1
kw	,
at	 
dv	2
kw	]

co	#+END_SRC

co	#+BEGIN_SRC json

kw	{
fu	"a"
kw	:
fu	 
kw	[
dv	1
kw	,
at	 
ch	true
kw	,
at	 
ch	null
kw	]}

in	#+END_SRC

"#,
    ),
    (
        r#"w10-fix/javascript tagged template"#,
        r#"javascript"#,
        false,
        r#"tag `x`; tag`y`; a.tag `z`; f (1); a.b`q`"#,
        r#"fu	tag
--	 
vs	`x`
op	;
--	 
fu	tag
vs	`y`
op	;
--	 a
op	.
fu	tag
--	 
vs	`z`
op	;
--	 
fu	f
--	 (
dv	1
--	)
op	;
--	 a
op	.
fu	b
vs	`q`

"#,
    ),
    (
        r#"w10-fix/javascriptreact jsx after colon"#,
        r#"javascriptreact"#,
        false,
        r#"const d = ok ? A : <B x="1" />;
const e = ok ? A /> : <B x="1" />;"#,
        r#"kw	const
--	 d 
op	=
--	 ok 
op	?
--	 A 
op	: 
fu	<B
--	 
ot	x
op	=
st	"1"
--	 
fu	/>
op	;

kw	const
--	 e 
op	=
--	 ok 
op	?
--	 A 
op	/>
--	 
op	: 
fu	<B
--	 
ot	x
op	=
st	"1"
--	 
fu	/>
op	;

"#,
    ),
    (
        r#"w10-fix/elixir at runs"#,
        r#"elixir"#,
        false,
        r#"x @1"z" @-x"z" @_a"z" @a?"z" @1-_"z" @1.2"z" @A.a"z""#,
        r#"--	x @1"z
st	" @-x"
--	z
st	" @_a"
--	z
st	" @a?"
--	z
st	" @1-_"
--	z
st	" @1.2"
--	z
st	" @A.a"
--	z
st	"

"#,
    ),
    (
        r#"w10-fix/elixireex at runs"#,
        r#"elixireex"#,
        false,
        r#"<%= @@@user %>
<%= a@user %>
<%= @user.name %>
<%= @1%> "z""#,
        r#"bu	<%=
--	 @@@user 
bu	%>

bu	<%=
--	 a
fu	@user
--	 
bu	%>

bu	<%=
--	 
fu	@user
--	.name 
bu	%>

bu	<%=
--	 @1%> 
st	"z"

"#,
    ),
    (
        r#"w10-fix/elixirheex values"#,
        r#"elixirheex"#,
        false,
        r#"<p>{@} a {@a}</p>
<p>{@@a} b</p>
<p classs="é>>Café 日本</p>
<div class="open
<div class="open
  more"#,
        r#"bu	<
kw	p
bu	>
st	{
--	@} a {
ot	@a
st	}
er	</
--	p
er	>

er	<
--	p
er	>
--	{@@a
st	}
--	 b
er	</
--	p
er	>

er	<
--	p classs
er	=
st	"é>>Café 日本</p>

st	<div class="
--	open

er	<
--	div class
er	=
st	"open

st	  more

"#,
    ),
    (
        r#"w10-fix/html values"#,
        r#"html"#,
        false,
        r#"<div class="a<b" id=c<d>
<p class='a</b' x=y</z>
<textarea>a<b &amp; c</textarea>"#,
        r#"dt	<
kw	div
ot	 class
op	=
st	"a
er	<
st	b"
ot	 id
op	=
st	c
er	<
st	d
dt	>

dt	<
kw	p
ot	 class
op	=
st	'a
er	<
st	/b'
ot	 x
op	=
st	y
er	<
st	/z
dt	>

dt	<
kw	textarea
dt	>
--	a<b 
dv	&amp;
--	 c
dt	</
kw	textarea
dt	>

"#,
    ),
    (
        r#"w10-fix/r unmatched"#,
        r#"r"#,
        false,
        r#"print(y))
{ }}
x[1]]"#,
        r#"fu	print
--	(y)
er	)

--	{ }
er	}

--	x[
dv	1
--	]]

"#,
    ),
    (
        r#"w10-fix/bash command words"#,
        r#"bash"#,
        false,
        r#""a", b
"a"x y
x"a", b
'a'b c
echo"a" b
"a";b c
for i in1 2 3; do echo; done
for i do x; done
int main(void) { return 0; }
echo a(b c)d e"#,
        r#"st	"a"
ex	,
--	 b

st	"a"
ex	x
--	 y

ex	x
st	"a"
ex	,
--	 b

st	'a'
ex	b
--	 c

ex	echo
st	"a"
--	 b

st	"a"
kw	;
ex	b
--	 c

cf	for
--	 i 
ex	in1
--	 2 3
kw	;
--	 
cf	do
--	 
bu	echo
kw	;
--	 
cf	done

cf	for
--	 i 
cf	do
--	 
ex	x
kw	;
--	 
cf	done

ex	int
--	 main
er	(
ex	void
kw	)
--	 
kw	{
--	 
cf	return
--	 
dv	0
kw	;
--	 
kw	}

bu	echo
--	 a
er	(
ex	b
--	 c
kw	)
ex	d
--	 e

"#,
    ),
    (
        r#"w10-fix/dockerfile cmd"#,
        r#"dockerfile"#,
        false,
        r#"CMD "python", "app.py"]
RUN for i in1 2 3; do \
      echo "$i"; \
    done"#,
        r#"kw	CMD
--	 
st	"python"
ex	,
--	 
st	"app.py"
--	]

kw	RUN
--	 
cf	for
--	 i 
ex	in1
--	 2 3
kw	;
--	 
cf	do
--	 
dt	\\

--	      
bu	echo
--	 
st	"
va	$i
st	"
kw	;
--	 
dt	\\

--	    
cf	done

"#,
    ),
    (
        r#"w10-fix/c directives"#,
        r#"c"#,
        false,
        r#"#+EN_SRC
#foo bar
#1 x
#
# x
#  define X 1
#IF x
#Pragma x
#include_next <a>"#,
        r#"pp	#
er	+EN_SRC

pp	#
er	foo bar

pp	#1 x

er	#

pp	# 
er	x

pp	#  define X 
dv	1

pp	#IF x

pp	#
er	Pragma x

pp	#include_next 
im	<a>

"#,
    ),
    (
        r#"w10-fix/cpp gcc and qt"#,
        r#"cpp"#,
        false,
        r#"struct __attribute__((aligned(8), unused)) S { __int128 x; };
if (__builtin_expect(x, 0)) __typeof__(x) y;
class W : public QObject { Q_OBJECT signals: void f(qint32); };
qDebug() << QString("a"); emit f(1);"#,
        r#"kw	struct
--	 
ex	__attribute__((aligned(8), unused))
--	 S 
op	{
--	 
dt	__int128
--	 x
op	;
--	 
op	};

cf	if
--	 
op	(
fu	__builtin_expect
op	(
--	x
op	,
--	 
dv	0
op	))
--	 
ex	__typeof__
op	(
--	x
op	)
--	 y
op	;

kw	class
--	 W 
op	:
--	 
kw	public
--	 
ex	QObject
--	 
op	{
--	 
ex	Q_OBJECT
--	 
ex	signals
op	:
--	 
dt	void
--	 f
op	(
ex	qint32
op	);
--	 
op	};

fu	qDebug
op	()
--	 
op	<<
--	 
ex	QString
op	(
st	"a"
op	);
--	 
ex	emit
--	 f
op	(
dv	1
op	);

"#,
    ),
    (
        r#"w10-fix/isocpp gcc"#,
        r#"isocpp"#,
        false,
        r#"struct __attribute__((packed)) S { __int128 x; };
class W : public QObject { Q_OBJECT };"#,
        r#"kw	struct
--	 
ex	__attribute__((packed))
--	 S 
op	{
--	 
dt	__int128
--	 x
op	;
--	 
op	};

kw	class
--	 W 
op	:
--	 
kw	public
--	 QObject 
op	{
--	 Q_OBJECT 
op	};

"#,
    ),
    (
        r#"w10-fix/gcc"#,
        r#"gcc"#,
        false,
        r#"struct __attribute__((packed)) S { __int128 x; }; int y = __builtin_foo(1) + __GNUC__;"#,
        r#"--	struct 
ex	__attribute__((packed))
--	 S { 
dt	__int128
--	 x; }; int y = 
fu	__builtin_foo
--	(1) + 
ot	__GNUC__
--	;

"#,
    ),
];

/// The token dump of highlighted lines. The writers merge neighbouring
/// `Normal` text, so the dump does too.
fn dump(lines: Option<Vec<SourceLine>>, inline: bool) -> String {
    let Some(lines) = lines else {
        return "NOT-HIGHLIGHTED\n".to_string();
    };
    let lines: Vec<SourceLine> = if inline {
        let mut one = Vec::new();
        for (i, line) in lines.into_iter().enumerate() {
            if i > 0 {
                one.push((TokenType::Normal, "\n".to_string()));
            }
            one.extend(line);
        }
        vec![one]
    } else {
        lines
    };
    let mut out = String::new();
    for line in lines {
        let mut tokens: Vec<(&str, String)> = Vec::new();
        for (t, text) in line {
            let class = match t.html_class() {
                "" => "--",
                class => class,
            };
            match tokens.last_mut() {
                _ if class == "--" && text.is_empty() => {}
                Some(last) if class == "--" && last.0 == "--" => last.1.push_str(&text),
                _ => tokens.push((class, text)),
            }
        }
        for (class, text) in tokens {
            let text = text.replace('\\', "\\\\").replace('\t', "\\t");
            out.push_str(&format!("{class}\t{text}\n"));
        }
        out.push('\n');
    }
    out
}

#[test]
fn clean_languages_give_the_tokens_of_the_oracle() {
    let registry = Registry::embedded();
    let mut failed = Vec::new();
    for (name, classes, inline, code, expected) in CASES {
        let classes: Vec<String> = classes.split(',').map(str::to_string).collect();
        let lines = registry
            .highlight_classes(&classes, code, 128)
            .and_then(Result::ok)
            .map(|c| c.lines);
        let got = dump(lines, *inline);
        if got.trim_end() != expected.trim_end() {
            failed.push(format!("{name}:\n{got}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn oversized_dynamic_delimiters_return_a_highlight_error() {
    let registry = Registry::embedded();
    let tag = "x".repeat(128 * 1024);
    let equals = "=".repeat(128 * 1024);
    for (language, code) in [
        ("lua", format!("local s = [{equals}[\nbody\n]{equals}]")),
        ("sqlpostgresql", format!("SELECT ${tag}$\nbody\n${tag}$;")),
        ("php", format!("<?php $x = <<<{tag}\nbody\n{tag};")),
        ("perl", format!("my $x = <<{tag};\nbody\n{tag}")),
    ] {
        let start = std::time::Instant::now();
        let result = registry.highlight_classes(&[language.to_string()], &code, 128);
        assert!(
            matches!(result, Some(Err(ref err)) if err.starts_with("Dynamic regular expression exceeds its budget")),
            "{language} did not reject its oversized delimiter"
        );
        eprintln!(
            "{language} rejected its 128 KiB delimiter in {:?}",
            start.elapsed()
        );
    }
}

#[test]
fn perl_transliteration_skips_escapes_and_reaches_later_tokens() -> Result<(), String> {
    let registry = Registry::embedded();
    let classes = ["perl".to_string()];
    for (search, replacement) in [(r"a\/b", r"c\/d"), (r"a\\", "b"), ("a", "b")] {
        let spans = format!("tr/{search}/{replacement}/");
        let code = format!("{spans}; print 1;");
        let output = registry
            .highlight_classes(&classes, &code, 128)
            .ok_or("Perl syntax is missing")??;
        assert_eq!(output.lines[0][0], (TokenType::Other, spans));
        assert!(output.lines[0].contains(&(TokenType::Function, "print".to_string())));
    }
    let search = r"a\/".repeat(16 * 1024);
    let code = format!("tr/{search}/b/; print 1;");
    let output = registry
        .highlight_classes(&classes, &code, 128)
        .ok_or("Perl syntax is missing")??;
    assert!(output.lines[0].contains(&(TokenType::Function, "print".to_string())));
    Ok(())
}

/// A developer aid for new cases: prints the token dump of each input of
/// the JSON file in `RANDOC_PROBE_INPUT` (a list of `[classes, code]`).
/// Run it with `cargo test -p randoc-highlight --test clean_languages --
/// --ignored --nocapture probe_inputs`.
#[test]
#[ignore]
fn probe_inputs() {
    let Ok(path) = std::env::var("RANDOC_PROBE_INPUT") else {
        return;
    };
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let inputs: Vec<(String, String)> = serde_json::from_str(&text).unwrap_or_default();
    let registry = Registry::embedded();
    let dumps: Vec<String> = inputs
        .iter()
        .map(|(classes, code)| {
            let classes: Vec<String> = classes.split(',').map(str::to_string).collect();
            let lines = registry
                .highlight_classes(&classes, code, 128)
                .and_then(Result::ok)
                .map(|c| c.lines);
            dump(lines, false)
        })
        .collect();
    println!(
        "PROBE-OUTPUT {}",
        serde_json::to_string(&dumps).unwrap_or_default()
    );
}
