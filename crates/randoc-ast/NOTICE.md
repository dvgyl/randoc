# Third-party notices for `randoc-ast`

This crate contains material from the projects below. Each one keeps its own
licence. The rest of the crate has the licence `MIT OR Apache-2.0`.

- The type names and the JSON form of the document model: `pandoc-types` 1.23.1.2 (BSD-3-Clause).
- The digit generation of `show` for `Double` (`floatToDigits`) and the `show` forms of `Double`, `Char` and `String`, ported to `src/json/haskell.rs`: GHC 9.12.2, library `base` (BSD-3-Clause).
- The tables of the general categories L and N in `src/json/categories.rs`, generated from `UnicodeData.txt`: Unicode Character Database 15.1.0 (Unicode License v3).

## `pandoc-types` 1.23.1.2

```text
Copyright (c) 2006-2023, John MacFarlane

All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

    * Redistributions of source code must retain the above copyright
      notice, this list of conditions and the following disclaimer.

    * Redistributions in binary form must reproduce the above
      copyright notice, this list of conditions and the following
      disclaimer in the documentation and/or other materials provided
      with the distribution.

    * Neither the name of John MacFarlane nor the names of other
      contributors may be used to endorse or promote products derived
      from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## GHC 9.12.2, library `base`

```text
This library (libraries/base) is derived from code from several
sources: 

  * Code from the GHC project which is largely (c) The University of
    Glasgow, and distributable under a BSD-style license (see below),

  * Code from the Haskell 98 Report which is (c) Simon Peyton Jones
    and freely redistributable (but see the full license for
    restrictions).

  * Code from the Haskell Foreign Function Interface specification,
    which is (c) Manuel M. T. Chakravarty and freely redistributable
    (but see the full license for restrictions).

The full text of these licenses is reproduced below.  All of the
licenses are BSD-style or compatible.

-----------------------------------------------------------------------------

The Glasgow Haskell Compiler License

Copyright 2004, The University Court of the University of Glasgow. 
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

- Redistributions of source code must retain the above copyright notice,
this list of conditions and the following disclaimer.
 
- Redistributions in binary form must reproduce the above copyright notice,
this list of conditions and the following disclaimer in the documentation
and/or other materials provided with the distribution.
 
- Neither name of the University nor the names of its contributors may be
used to endorse or promote products derived from this software without
specific prior written permission. 

THIS SOFTWARE IS PROVIDED BY THE UNIVERSITY COURT OF THE UNIVERSITY OF
GLASGOW AND THE CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
UNIVERSITY COURT OF THE UNIVERSITY OF GLASGOW OR THE CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH
DAMAGE.

-----------------------------------------------------------------------------

Code derived from the document "Report on the Programming Language
Haskell 98", is distributed under the following license:

  Copyright (c) 2002 Simon Peyton Jones

  The authors intend this Report to belong to the entire Haskell
  community, and so we grant permission to copy and distribute it for
  any purpose, provided that it is reproduced in its entirety,
  including this Notice.  Modified versions of this Report may also be
  copied and distributed for any purpose, provided that the modified
  version is clearly presented as such, and that it does not claim to
  be a definition of the Haskell 98 Language.

-----------------------------------------------------------------------------

Code derived from the document "The Haskell 98 Foreign Function
Interface, An Addendum to the Haskell 98 Report" is distributed under
the following license:

  Copyright (c) 2002 Manuel M. T. Chakravarty

  The authors intend this Report to belong to the entire Haskell
  community, and so we grant permission to copy and distribute it for
  any purpose, provided that it is reproduced in its entirety,
  including this Notice.  Modified versions of this Report may also be
  copied and distributed for any purpose, provided that the modified
  version is clearly presented as such, and that it does not claim to
  be a definition of the Haskell 98 Foreign Function Interface.

-----------------------------------------------------------------------------
```

## Unicode Character Database 15.1.0

```text
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 1991-2026 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.
```
