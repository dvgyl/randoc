//! URI validation for Markdown autolinks.

/// Returns the text that an autolink would display.
pub(crate) fn autolink_label(url: &str) -> Option<&str> {
    let (scheme, body) = url.split_once(':')?;
    let lower = scheme.to_ascii_lowercase();
    if SCHEMES.binary_search(&lower.as_str()).is_err() {
        return None;
    }
    let (path, fragment) = body.split_once('#').unwrap_or((body, ""));
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let sub_delims = "!$&'()*+,;=";
    if !valid_chars(fragment, "!$&'()*+,;=:@/?") || !valid_chars(query, "!$&'()*+,;=:@/?") {
        return None;
    }
    let path = if let Some(authority_and_path) = path.strip_prefix("//") {
        let (authority, path) = authority_and_path
            .split_once('/')
            .unwrap_or((authority_and_path, ""));
        let host_port = if let Some((user, host_port)) = authority.split_once('@') {
            if !valid_chars(user, "!$&'()*+,;=:") {
                return None;
            }
            host_port
        } else {
            authority
        };
        let (host, port) = host_port.split_once(':').unwrap_or((host_port, ""));
        if !valid_chars(host, sub_delims) || !port.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        path
    } else {
        path
    };
    if !valid_chars(path, "!$&'()*+,;=:@/") {
        return None;
    }
    Some(if scheme == "mailto" { body } else { url })
}

/// Percent-encodes the white space and the characters `<>|"{}[]^`` ` ``
/// of a URL, as the Markdown reader does with the URL of a link.
pub(crate) fn escape_uri(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for c in url.chars() {
        if crate::inlines::is_haskell_space(c) || "<>|\"{}[]^`".contains(c) {
            let mut buf = [0u8; 4];
            for byte in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn valid_chars(text: &str, delimiters: &str) -> bool {
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            if !chars.next().is_some_and(|h| h.is_ascii_hexdigit())
                || !chars.next().is_some_and(|h| h.is_ascii_hexdigit())
            {
                return false;
            }
        } else if !(c.is_ascii_alphanumeric()
            || "-._~".contains(c)
            || delimiters.contains(c)
            || !c.is_ascii() && !c.is_control() && !c.is_whitespace())
        {
            return false;
        }
    }
    true
}

// The schemes accepted by Pandoc 3.13 in autolink probes.
const SCHEMES: &[&str] = &[
    "aaa",
    "aaas",
    "about",
    "acap",
    "acct",
    "acr",
    "adiumxtra",
    "afp",
    "afs",
    "aim",
    "appdata",
    "apt",
    "attachment",
    "aw",
    "barion",
    "beshare",
    "bitcoin",
    "blob",
    "bolo",
    "browserext",
    "callto",
    "cap",
    "chrome",
    "chrome-extension",
    "cid",
    "coap",
    "coaps",
    "com-eventbrite-attendee",
    "content",
    "crid",
    "cvs",
    "data",
    "dav",
    "dict",
    "dis",
    "dlna-playcontainer",
    "dlna-playsingle",
    "dns",
    "dntp",
    "doi",
    "dtn",
    "dvb",
    "ed2k",
    "example",
    "facetime",
    "fax",
    "feed",
    "feedready",
    "file",
    "filesystem",
    "finger",
    "fish",
    "ftp",
    "gemini",
    "geo",
    "gg",
    "git",
    "gizmoproject",
    "go",
    "gopher",
    "graph",
    "gtalk",
    "h323",
    "ham",
    "hcp",
    "http",
    "https",
    "hxxp",
    "hxxps",
    "hydrazone",
    "iax",
    "icap",
    "icon",
    "im",
    "imap",
    "info",
    "iotdisco",
    "ipn",
    "ipp",
    "ipps",
    "irc",
    "irc6",
    "ircs",
    "iris",
    "iris.beep",
    "iris.lwz",
    "iris.xpc",
    "iris.xpcs",
    "isbn",
    "isostore",
    "itms",
    "jabber",
    "jar",
    "javascript",
    "jms",
    "keyparc",
    "lastfm",
    "ldap",
    "ldaps",
    "lvlt",
    "magnet",
    "mailserver",
    "mailto",
    "maps",
    "market",
    "message",
    "mid",
    "mms",
    "modem",
    "mongodb",
    "moz",
    "ms-access",
    "ms-browser-extension",
    "ms-drive-to",
    "ms-enrollment",
    "ms-excel",
    "ms-gamebarservices",
    "ms-getoffice",
    "ms-help",
    "ms-infopath",
    "ms-media-stream-id",
    "ms-officeapp",
    "ms-powerpoint",
    "ms-project",
    "ms-publisher",
    "ms-search-repair",
    "ms-secondary-screen-controller",
    "ms-secondary-screen-setup",
    "ms-settings",
    "ms-settings-airplanemode",
    "ms-settings-bluetooth",
    "ms-settings-camera",
    "ms-settings-cellular",
    "ms-settings-cloudstorage",
    "ms-settings-connectabledevices",
    "ms-settings-displays-topology",
    "ms-settings-emailandaccounts",
    "ms-settings-language",
    "ms-settings-location",
    "ms-settings-lock",
    "ms-settings-nfctransactions",
    "ms-settings-notifications",
    "ms-settings-power",
    "ms-settings-privacy",
    "ms-settings-proximity",
    "ms-settings-screenrotation",
    "ms-settings-wifi",
    "ms-settings-workplace",
    "ms-spd",
    "ms-sttoverlay",
    "ms-transit-to",
    "ms-virtualtouchpad",
    "ms-visio",
    "ms-walk-to",
    "ms-whiteboard",
    "ms-whiteboard-cmd",
    "ms-word",
    "msnim",
    "msrp",
    "msrps",
    "mtqp",
    "mumble",
    "mupdate",
    "mvn",
    "news",
    "nfs",
    "ni",
    "nih",
    "nntp",
    "notes",
    "ocf",
    "oid",
    "onenote",
    "onenote-cmd",
    "opaquelocktoken",
    "pack",
    "palm",
    "paparazzi",
    "pkcs11",
    "platform",
    "pmid",
    "pop",
    "pres",
    "prospero",
    "proxy",
    "psyc",
    "pwid",
    "qb",
    "query",
    "redis",
    "rediss",
    "reload",
    "res",
    "resource",
    "rmi",
    "rsync",
    "rtmfp",
    "rtmp",
    "rtsp",
    "rtsps",
    "rtspu",
    "secondlife",
    "service",
    "session",
    "sftp",
    "sgn",
    "shttp",
    "sieve",
    "sip",
    "sips",
    "skype",
    "smb",
    "sms",
    "smtp",
    "snews",
    "snmp",
    "soap.beep",
    "soap.beeps",
    "soldat",
    "spotify",
    "ssh",
    "steam",
    "stun",
    "stuns",
    "submit",
    "svn",
    "tag",
    "teamspeak",
    "tel",
    "teliaeid",
    "telnet",
    "tftp",
    "things",
    "thismessage",
    "tip",
    "tn3270",
    "tool",
    "turn",
    "turns",
    "tv",
    "udp",
    "unreal",
    "urn",
    "ut2004",
    "v-event",
    "vemmi",
    "ventrilo",
    "videotex",
    "view-source",
    "vnc",
    "wais",
    "webcal",
    "wpid",
    "ws",
    "wss",
    "wtai",
    "wyciwyg",
    "xcon",
    "xcon-userid",
    "xfire",
    "xmlrpc.beep",
    "xmlrpc.beeps",
    "xmpp",
    "xri",
    "ymsgr",
    "z39.50",
    "z39.50r",
    "z39.50s",
];
