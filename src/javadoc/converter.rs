//! Ports of jdt.ls `JavaDoc2MarkdownConverter` and `JavaDoc2PlainTextConverter`
//! (and their base `AbstractJavaDocConverter`).
//!
//! Input is raw Javadoc text (or HTML): it first goes through the
//! `JdtLsJavaDoc2HTMLTextReader` port, then is parsed as HTML (jsoup port),
//! sanitised (`TableHelper`, consecutive code tags) and converted with the
//! flexmark html2md port.

use super::html::{self, Document, NodeId};
use super::html2md::convert_document;
use super::plain_text::get_plain_text;
use super::table_helper::normalize_table_headers;
use super::text_reader::{java_is_whitespace, javadoc_to_html};
use super::SNIPPET;

const MARKDOWN_SPACE: &str = "&nbsp;";
const DOUBLE_SPACE: &str = "  ";

/// `new JavaDoc2MarkdownConverter(javadoc).getAsString()`
pub fn javadoc_to_markdown(javadoc: Option<&str>) -> Option<String> {
    let raw_html = javadoc_to_html(javadoc?);
    Some(html_to_markdown(&raw_html))
}

/// `new JavaDoc2PlainTextConverter(javadoc).getAsString()`
pub fn javadoc_to_plain_text(javadoc: Option<&str>) -> Option<String> {
    let raw_html = javadoc_to_html(javadoc?);
    let doc = html::parse(&raw_html);
    Some(get_plain_text(&doc, html::ROOT))
}

/// `JavaDoc2MarkdownConverter.convert(html)`
pub fn html_to_markdown(html_text: &str) -> String {
    let mut doc = html::parse(html_text);
    sanitize(&mut doc);
    let markdown = convert_document(&doc, -1);
    fix_snippet(&markdown)
}

fn sanitize(doc: &mut Document) {
    for table in doc.select_tag(html::ROOT, "table") {
        normalize_table_headers(doc, table);
    }
    separate_consecutive_code_tags(doc);
}

fn is_code_tag(doc: &Document, n: NodeId) -> bool {
    let t = doc.tag_name(n);
    t == "tt" || t == "code"
}

fn separate_consecutive_code_tags(doc: &mut Document) {
    let codes: Vec<NodeId> = doc.descendants_and_self(html::ROOT).into_iter().filter(|e| is_code_tag(doc, *e)).collect();
    for code in codes {
        if let Some(next_el) = doc.next_element_sibling(code) {
            if is_code_tag(doc, next_el) && doc.next_sibling(code) == Some(next_el) {
                let space = doc.create_leaf(html::Kind::Text, " ");
                doc.insert_after(code, space);
            }
        }
    }
}

/// Java `String.lines()`
fn java_lines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\n' || b[i] == b'\r' {
            out.push(&s[start..i]);
            if b[i] == b'\r' && i + 1 < b.len() && b[i + 1] == b'\n' {
                i += 1;
            }
            start = i + 1;
        }
        i += 1;
    }
    if start < b.len() {
        out.push(&s[start..]);
    }
    out
}

fn fix_snippet(value: &str) -> String {
    if !value.contains(SNIPPET) {
        return value.to_string();
    }
    let mut builder = String::new();
    for line in java_lines(value) {
        let mut line = line.to_string();
        if line.contains(SNIPPET) {
            line = line.trim_start_matches(java_is_whitespace).to_string();
            if line.starts_with(SNIPPET) {
                line = line.replacen(SNIPPET, "", 1);
                line = replace_leading_spaces(&line);
                if !line.ends_with(DOUBLE_SPACE) {
                    line.push_str(DOUBLE_SPACE);
                }
            }
        }
        builder.push_str(&line);
        builder.push('\n');
    }
    builder
}

fn replace_leading_spaces(s: &str) -> String {
    let mut s: Vec<char> = s.chars().collect();
    let mut i = 0;
    while s.len() > i + 1 && s[i] == ' ' {
        // str.replaceFirst(" ", MARKDOWN_SPACE): the first space is at index i
        let pos = s.iter().position(|c| *c == ' ').unwrap();
        s.splice(pos..pos + 1, MARKDOWN_SPACE.chars());
        i += MARKDOWN_SPACE.len();
    }
    s.into_iter().collect()
}

#[cfg(test)]
mod abstract_javadoc_converter_test {
    //! Shared constants of upstream `AbstractJavadocConverterTest`.
    #![allow(dead_code)]
    pub const RAW_JAVADOC_0: &str = "This Javadoc  contains some <code> code </code>, a link to {@link IOException} and a table\n<table>\n  <thead><tr><th>header 1</th><th>header 2</th></tr></thead>\n  <tbody><tr><td>data 1</td><td>data 2</td></tr></tbody>\n  </table>\n<br> literally {@literal <b>literal</b>} and now a list:\n  <ul>\n<li><b>Coffee</b>\n   <ul>\n    <li>Mocha</li>\n    <li>Latte</li>\n   </ul>\n  </li>\n  <li>Tea\n   <ul>\n    <li>Darjeeling</li>\n    <li>Early Grey</li>\n   </ul>\n  </li>\n</ul>\n\n @param param1 the first parameter\n @param param2\n the 2nd parameter\n @param param3\n @since 1.0\n @since .0\n @author <a href=\"mailto:foo@bar.com\">Ralf</a>\n @author <a href=\"mailto:bar@foo.com\">Andrew</a>\n @exception NastyException a\n nasty exception\n @throws\nIOException another nasty exception\n @return some kind of result\n @unknown unknown tag\n @unknown another unknown tag\n";
    pub const RAW_JAVADOC_TABLE_0: &str = "<table>\n    <tr>\n        <th>Header 1</th>\n        <th>Header 2</th>\n    </tr>\n    <tr>\n        <td>Row 1A</td>\n        <td>Row 1B</td>\n    </tr>\n    <tr>\n        <td>Row 2A</td>\n        <td>Row 2B</td>\n    </tr>\n</table>";
    pub const RAW_JAVADOC_TABLE_1: &str = "<table>\n    <tr>\n        <td>Row 0A</td>\n        <td>Row 0B</td>\n    </tr>\n    <tr>\n        <td>Row 1A</td>\n        <td>Row 1B</td>\n    </tr>\n    <tr>\n        <td>Row 2A</td>\n        <td>Row 2B</td>\n    </tr>\n</table>";
}

#[cfg(test)]
mod java_doc2_markdown_converter_test {
    //! Port of upstream `JavaDoc2MarkdownConverterTest`.
    use super::abstract_javadoc_converter_test::*;
    use super::javadoc_to_markdown;

    fn independent(s: &str) -> String {
        s.replace("\r\n", "\n").replace('\r', "\n")
    }

    fn md(s: &str) -> String {
        javadoc_to_markdown(Some(s)).unwrap()
    }

    fn extract_label_and_uri_from_link_markdown(markdown: &str) -> (String, String) {
        if markdown.is_empty() {
            return (String::new(), String::new());
        }
        let re = regex::Regex::new(r"\[(.*?)\]\((.*?)\)").unwrap();
        match re.captures(markdown) {
            Some(c) => (c[1].to_string(), c[2].to_string()),
            None => (String::new(), String::new()),
        }
    }

    const MARKDOWN_0: &str = "This Javadoc contains some ` code `, a link to `IOException` and a table\n\n| header 1 | header 2 |\n|----------|----------|\n| data 1   | data 2   |\n\n<br />\n\nliterally \\<b\\>literal\\</b\\> and now a list:\n\n* **Coffee**\n  * Mocha\n  * Latte\n* Tea\n  * Darjeeling\n  * Early Grey\n\n<!-- -->\n\n* **Parameters:**\n  * **param1** the first parameter\n  * **param2** the 2nd parameter\n  * **param3**\n* **Returns:**\n  * some kind of result\n* **Throws:**\n  * NastyException a nasty exception\n  * IOException another nasty exception\n* **Author:**\n  * [Ralf](mailto:foo@bar.com)\n  * [Andrew](mailto:bar@foo.com)\n* **Since:**\n  * 1.0\n  * 0\n* @unknown\n  * unknown tag\n* @unknown\n  * another unknown tag";
    const MARKDOWN_TABLE_0: &str = "| Header 1 | Header 2 |\n|----------|----------|\n| Row 1A   | Row 1B   |\n| Row 2A   | Row 2B   |";
    const MARKDOWN_TABLE_1: &str = "|        |        |\n|--------|--------|\n| Row 0A | Row 0B |\n| Row 1A | Row 1B |\n| Row 2A | Row 2B |";
    const RAW_JAVADOC_HTML_1: &str = "<a href=\"file://some_location\">File</a>";
    const RAW_JAVADOC_HTML_2: &str = "<a href=\"jdt://some_location\">JDT</a>";
    const RAW_JAVADOC_HTML_SEE: &str = "@see <a href=\"https://docs.oracle.com/javase/7/docs/api/\">Online docs for java</a>";
    const RAW_JAVADOC_HTML_PARAM: &str = "@param someString the string to enter";
    const RAW_JAVADOC_HTML_SINCE: &str = "@since 0.0.1";
    const RAW_JAVADOC_HTML_VERSION: &str = "@version 0.0.1";
    const RAW_JAVADOC_HTML_THROWS: &str = "@throws IOException";
    const RAW_JAVADOC_HTML_AUTHOR: &str = "@author author one\n@author author two\n@author author three\n";
    const RAW_JAVADOC_HTML_MIX: &str = "A super important method.\n\n@see java.lang.String#split(String, int)\n@see java.lang.String#split(String)\n@author      JSR-666 Expert Group\n@author      Chuck Norris\n@since       1.666\n@spec        JSR-666";
    const CHARSET_HTML_JAVADOC: &str = "<div class=\"block\">A named mapping between sequences of sixteen-bit Unicode <a href=\"../../lang/Character.html#unicode\">code units</a> and sequences of\nbytes.  This class defines methods for creating decoders and encoders and\nfor retrieving the various names associated with a charset.  Instances of\nthis class are immutable.\n\n<p> This class also defines static methods for testing whether a particular\ncharset is supported, for locating charset instances by name, and for\nconstructing a map that contains every charset for which support is\navailable in the current Java virtual machine.  Support for new charsets can\nbe added via the service-provider interface defined in the <a href=\"../../../java/nio/charset/spi/CharsetProvider.html\" title=\"class in java.nio.charset.spi\"><code>CharsetProvider</code></a> class.\n\n</p><p> All of the methods defined in this class are safe for use by multiple\nconcurrent threads.\n\n\n<a name=\"names\"></a><a name=\"charenc\"></a>\n</p><h2>Charset names</h2>\n\n<p> Charsets are named by strings composed of the following characters:\n\n</p><ul>\n\n  <li> The uppercase letters <tt>'A'</tt> through <tt>'Z'</tt>\n       (<tt>'A'</tt>&nbsp;through&nbsp;<tt>'Z'</tt>),\n\n  </li><li> The lowercase letters <tt>'a'</tt> through <tt>'z'</tt>\n       (<tt>'a'</tt>&nbsp;through&nbsp;<tt>'z'</tt>),\n\n  </li><li> The digits <tt>'0'</tt> through <tt>'9'</tt>\n       (<tt>'0'</tt>&nbsp;through&nbsp;<tt>'9'</tt>),\n\n  </li><li> The dash character <tt>'-'</tt>\n       (<tt>'-'</tt>,&nbsp;<small>HYPHEN-MINUS</small>),\n\n  </li><li> The plus character <tt>'+'</tt>\n       (<tt>'+'</tt>,&nbsp;<small>PLUS SIGN</small>),\n\n  </li><li> The period character <tt>'.'</tt>\n       (<tt>'.'</tt>,&nbsp;<small>FULL STOP</small>),\n\n  </li><li> The colon character <tt>':'</tt>\n       (<tt>':'</tt>,&nbsp;<small>COLON</small>), and\n\n  </li><li> The underscore character <tt>'_'</tt>\n       (<tt>'_'</tt>,&nbsp;<small>LOW&nbsp;LINE</small>).\n\n</li></ul>\n\nA charset name must begin with either a letter or a digit.  The empty string\nis not a legal charset name.  Charset names are not case-sensitive; that is,\ncase is always ignored when comparing charset names.  Charset names\ngenerally follow the conventions documented in <a href=\"http://www.ietf.org/rfc/rfc2278.txt\"><i>RFC&nbsp;2278:&nbsp;IANA Charset\nRegistration Procedures</i></a>.\n\n<p></p><p> Every charset has a <i>canonical name</i> and may also have one or more\n<i>aliases</i>.  The canonical name is returned by the <a href=\"../../../java/nio/charset/Charset.html#name--\"><code>name</code></a> method\nof this class.  Canonical names are, by convention, usually in upper case.\nThe aliases of a charset are returned by the <a href=\"../../../java/nio/charset/Charset.html#aliases--\"><code>aliases</code></a>\nmethod.\n\n</p><p><a name=\"hn\">Some charsets have an <i>historical name</i> that is defined for\ncompatibility with previous versions of the Java platform.</a>  A charset's\nhistorical name is either its canonical name or one of its aliases.  The\nhistorical name is returned by the <tt>getEncoding()</tt> methods of the\n<a href=\"../../../java/io/InputStreamReader.html#getEncoding--\"><code>InputStreamReader</code></a> and <a href=\"../../../java/io/OutputStreamWriter.html#getEncoding--\"><code>OutputStreamWriter</code></a> classes.\n\n</p><p><a name=\"iana\"> </a>If a charset listed in the <a href=\"http://www.iana.org/assignments/character-sets\"><i>IANA Charset\nRegistry</i></a> is supported by an implementation of the Java platform then\nits canonical name must be the name listed in the registry. Many charsets\nare given more than one name in the registry, in which case the registry\nidentifies one of the names as <i>MIME-preferred</i>.  If a charset has more\nthan one registry name then its canonical name must be the MIME-preferred\nname and the other names in the registry must be valid aliases.  If a\nsupported charset is not listed in the IANA registry then its canonical name\nmust begin with one of the strings <tt>\"X-\"</tt> or <tt>\"x-\"</tt>.\n\n</p><p> The IANA charset registry does change over time, and so the canonical\nname and the aliases of a particular charset may also change over time.  To\nensure compatibility it is recommended that no alias ever be removed from a\ncharset, and that if the canonical name of a charset is changed then its\nprevious canonical name be made into an alias.\n\n\n</p><h2>Standard charsets</h2>\n\n\n\n<p><a name=\"standard\">Every implementation of the Java platform is required to support the\nfollowing standard charsets.</a>  Consult the release documentation for your\nimplementation to see if any other charsets are supported.  The behavior\nof such optional charsets may differ between implementations.\n\n</p><blockquote><table width=\"80%\" summary=\"Description of standard charsets\">\n<tbody><tr><th align=\"left\">Charset</th><th align=\"left\">Description</th></tr>\n<tr><td valign=\"top\"><tt>US-ASCII</tt></td>\n    <td>Seven-bit ASCII, a.k.a. <tt>ISO646-US</tt>,\n        a.k.a. the Basic Latin block of the Unicode character set</td></tr>\n<tr><td valign=\"top\"><tt>ISO-8859-1&nbsp;&nbsp;</tt></td>\n    <td>ISO Latin Alphabet No. 1, a.k.a. <tt>ISO-LATIN-1</tt></td></tr>\n<tr><td valign=\"top\"><tt>UTF-8</tt></td>\n    <td>Eight-bit UCS Transformation Format</td></tr>\n<tr><td valign=\"top\"><tt>UTF-16BE</tt></td>\n    <td>Sixteen-bit UCS Transformation Format,\n        big-endian byte&nbsp;order</td></tr>\n<tr><td valign=\"top\"><tt>UTF-16LE</tt></td>\n    <td>Sixteen-bit UCS Transformation Format,\n        little-endian byte&nbsp;order</td></tr>\n<tr><td valign=\"top\"><tt>UTF-16</tt></td>\n    <td>Sixteen-bit UCS Transformation Format,\n        byte&nbsp;order identified by an optional byte-order mark</td></tr>\n</tbody></table></blockquote>\n\n<p></p><p> The <tt>UTF-8</tt> charset is specified by <a href=\"http://www.ietf.org/rfc/rfc2279.txt\"><i>RFC&nbsp;2279</i></a>; the\ntransformation format upon which it is based is specified in\nAmendment&nbsp;2 of ISO&nbsp;10646-1 and is also described in the <a href=\"http://www.unicode.org/unicode/standard/standard.html\"><i>Unicode\nStandard</i></a>.\n\n</p><p> The <tt>UTF-16</tt> charsets are specified by <a href=\"http://www.ietf.org/rfc/rfc2781.txt\"><i>RFC&nbsp;2781</i></a>; the\ntransformation formats upon which they are based are specified in\nAmendment&nbsp;1 of ISO&nbsp;10646-1 and are also described in the <a href=\"http://www.unicode.org/unicode/standard/standard.html\"><i>Unicode\nStandard</i></a>.\n\n</p><p> The <tt>UTF-16</tt> charsets use sixteen-bit quantities and are\ntherefore sensitive to byte order.  In these encodings the byte order of a\nstream may be indicated by an initial <i>byte-order mark</i> represented by\nthe Unicode character <tt>'\u{feff}'</tt>.  Byte-order marks are handled\nas follows:\n\n</p><ul>\n\n  <li><p> When decoding, the <tt>UTF-16BE</tt> and <tt>UTF-16LE</tt>\n  charsets interpret the initial byte-order marks as a <small>ZERO-WIDTH\n  NON-BREAKING SPACE</small>; when encoding, they do not write\n  byte-order marks. </p></li>\n\n\n  <li><p> When decoding, the <tt>UTF-16</tt> charset interprets the\n  byte-order mark at the beginning of the input stream to indicate the\n  byte-order of the stream but defaults to big-endian if there is no\n  byte-order mark; when encoding, it uses big-endian byte order and writes\n  a big-endian byte-order mark. </p></li>\n\n</ul>\n\nIn any case, byte order marks occurring after the first element of an\ninput sequence are not omitted since the same code is used to represent\n<small>ZERO-WIDTH NON-BREAKING SPACE</small>.\n\n<p></p><p> Every instance of the Java virtual machine has a default charset, which\nmay or may not be one of the standard charsets.  The default charset is\ndetermined during virtual-machine startup and typically depends upon the\nlocale and charset being used by the underlying operating system. </p>\n\n<p>The <a href=\"../../../java/nio/charset/StandardCharsets.html\" title=\"class in java.nio.charset\"><code>StandardCharsets</code></a> class defines constants for each of the\nstandard charsets.\n\n</p><h2>Terminology</h2>\n\n<p> The name of this class is taken from the terms used in\n<a href=\"http://www.ietf.org/rfc/rfc2278.txt\"><i>RFC&nbsp;2278</i></a>.\nIn that document a <i>charset</i> is defined as the combination of\none or more coded character sets and a character-encoding scheme.\n(This definition is confusing; some other software systems define\n<i>charset</i> as a synonym for <i>coded character set</i>.)\n\n</p><p> A <i>coded character set</i> is a mapping between a set of abstract\ncharacters and a set of integers.  US-ASCII, ISO&nbsp;8859-1,\nJIS&nbsp;X&nbsp;0201, and Unicode are examples of coded character sets.\n\n</p><p> Some standards have defined a <i>character set</i> to be simply a\nset of abstract characters without an associated assigned numbering.\nAn alphabet is an example of such a character set.  However, the subtle\ndistinction between <i>character set</i> and <i>coded character set</i>\nis rarely used in practice; the former has become a short form for the\nlatter, including in the Java API specification.\n\n</p><p> A <i>character-encoding scheme</i> is a mapping between one or more\ncoded character sets and a set of octet (eight-bit byte) sequences.\nUTF-8, UTF-16, ISO&nbsp;2022, and EUC are examples of\ncharacter-encoding schemes.  Encoding schemes are often associated with\na particular coded character set; UTF-8, for example, is used only to\nencode Unicode.  Some schemes, however, are associated with multiple\ncoded character sets; EUC, for example, can be used to encode\ncharacters in a variety of Asian coded character sets.\n\n</p><p> When a coded character set is used exclusively with a single\ncharacter-encoding scheme then the corresponding charset is usually\nnamed for the coded character set; otherwise a charset is usually named\nfor the encoding scheme and, possibly, the locale of the coded\ncharacter sets that it supports.  Hence <tt>US-ASCII</tt> is both the\nname of a coded character set and of the charset that encodes it, while\n<tt>EUC-JP</tt> is the name of the charset that encodes the\nJIS&nbsp;X&nbsp;0201, JIS&nbsp;X&nbsp;0208, and JIS&nbsp;X&nbsp;0212\ncoded character sets for the Japanese language.\n\n</p><p> The native character encoding of the Java programming language is\nUTF-16.  A charset in the Java platform therefore defines a mapping\nbetween sequences of sixteen-bit UTF-16 code units (that is, sequences\nof chars) and sequences of bytes. </p></div>\n";
    const CHARSET_MD_JAVADOC: &str = "A named mapping between sequences of sixteen-bit Unicode [code units](../../lang/Character.html#unicode) and sequences of bytes. This class defines methods for creating decoders and encoders and for retrieving the various names associated with a charset. Instances of this class are immutable.\n\nThis class also defines static methods for testing whether a particular\ncharset is supported, for locating charset instances by name, and for\nconstructing a map that contains every charset for which support is\navailable in the current Java virtual machine. Support for new charsets can\nbe added via the service-provider interface defined in the [`CharsetProvider`](../../../java/nio/charset/spi/CharsetProvider.html \"class in java.nio.charset.spi\") class.\n\nAll of the methods defined in this class are safe for use by multiple\nconcurrent threads.\n\n\nCharset names\n-------------\n\nCharsets are named by strings composed of the following characters:\n\n* The uppercase letters `'A'` through `'Z'` (`'A'` through `'Z'`),\n* The lowercase letters `'a'` through `'z'` (`'a'` through `'z'`),\n* The digits `'0'` through `'9'` (`'0'` through `'9'`),\n* The dash character `'-'` (`'-'`, HYPHEN-MINUS),\n* The plus character `'+'` (`'+'`, PLUS SIGN),\n* The period character `'.'` (`'.'`, FULL STOP),\n* The colon character `':'` (`':'`, COLON), and\n* The underscore character `'_'` (`'_'`, LOW LINE).\n\nA charset name must begin with either a letter or a digit. The empty string is not a legal charset name. Charset names are not case-sensitive; that is, case is always ignored when comparing charset names. Charset names generally follow the conventions documented in [*RFC 2278: IANA Charset\nRegistration Procedures*](http://www.ietf.org/rfc/rfc2278.txt).\n\n<br />\n\nEvery charset has a *canonical name* and may also have one or more\n*aliases* . The canonical name is returned by the [`name`](../../../java/nio/charset/Charset.html#name--) method\nof this class. Canonical names are, by convention, usually in upper case.\nThe aliases of a charset are returned by the [`aliases`](../../../java/nio/charset/Charset.html#aliases--)\nmethod.\n\nSome charsets have an *historical name* that is defined for\ncompatibility with previous versions of the Java platform. A charset's\nhistorical name is either its canonical name or one of its aliases. The\nhistorical name is returned by the `getEncoding()` methods of the\n[`InputStreamReader`](../../../java/io/InputStreamReader.html#getEncoding--) and [`OutputStreamWriter`](../../../java/io/OutputStreamWriter.html#getEncoding--) classes.\n\nIf a charset listed in the [*IANA Charset\nRegistry*](http://www.iana.org/assignments/character-sets) is supported by an implementation of the Java platform then\nits canonical name must be the name listed in the registry. Many charsets\nare given more than one name in the registry, in which case the registry\nidentifies one of the names as *MIME-preferred* . If a charset has more\nthan one registry name then its canonical name must be the MIME-preferred\nname and the other names in the registry must be valid aliases. If a\nsupported charset is not listed in the IANA registry then its canonical name\nmust begin with one of the strings `\"X-\"` or `\"x-\"`.\n\nThe IANA charset registry does change over time, and so the canonical\nname and the aliases of a particular charset may also change over time. To\nensure compatibility it is recommended that no alias ever be removed from a\ncharset, and that if the canonical name of a charset is changed then its\nprevious canonical name be made into an alias.\n\n\nStandard charsets\n-----------------\n\nEvery implementation of the Java platform is required to support the\nfollowing standard charsets. Consult the release documentation for your\nimplementation to see if any other charsets are supported. The behavior\nof such optional charsets may differ between implementations.\n\n> | Charset       | Description                                                                                    |\n> |:--------------|:-----------------------------------------------------------------------------------------------|\n> | `US-ASCII`    | Seven-bit ASCII, a.k.a. `ISO646-US`, a.k.a. the Basic Latin block of the Unicode character set |\n> | `ISO-8859-1 ` | ISO Latin Alphabet No. 1, a.k.a. `ISO-LATIN-1`                                                 |\n> | `UTF-8`       | Eight-bit UCS Transformation Format                                                            |\n> | `UTF-16BE`    | Sixteen-bit UCS Transformation Format, big-endian byte order                                   |\n> | `UTF-16LE`    | Sixteen-bit UCS Transformation Format, little-endian byte order                                |\n> | `UTF-16`      | Sixteen-bit UCS Transformation Format, byte order identified by an optional byte-order mark    |\n\n<br />\n\nThe `UTF-8` charset is specified by [*RFC 2279*](http://www.ietf.org/rfc/rfc2279.txt); the\ntransformation format upon which it is based is specified in\nAmendment 2 of ISO 10646-1 and is also described in the [*Unicode\nStandard*](http://www.unicode.org/unicode/standard/standard.html).\n\nThe `UTF-16` charsets are specified by [*RFC 2781*](http://www.ietf.org/rfc/rfc2781.txt); the\ntransformation formats upon which they are based are specified in\nAmendment 1 of ISO 10646-1 and are also described in the [*Unicode\nStandard*](http://www.unicode.org/unicode/standard/standard.html).\n\nThe `UTF-16` charsets use sixteen-bit quantities and are\ntherefore sensitive to byte order. In these encodings the byte order of a\nstream may be indicated by an initial *byte-order mark* represented by\nthe Unicode character `'\u{feff}'`. Byte-order marks are handled\nas follows:\n\n* When decoding, the `UTF-16BE` and `UTF-16LE`\n  charsets interpret the initial byte-order marks as a ZERO-WIDTH NON-BREAKING SPACE; when encoding, they do not write\n  byte-order marks.\n\n* When decoding, the `UTF-16` charset interprets the\n  byte-order mark at the beginning of the input stream to indicate the\n  byte-order of the stream but defaults to big-endian if there is no\n  byte-order mark; when encoding, it uses big-endian byte order and writes\n  a big-endian byte-order mark.\n\nIn any case, byte order marks occurring after the first element of an input sequence are not omitted since the same code is used to represent ZERO-WIDTH NON-BREAKING SPACE.\n\n<br />\n\nEvery instance of the Java virtual machine has a default charset, which\nmay or may not be one of the standard charsets. The default charset is\ndetermined during virtual-machine startup and typically depends upon the\nlocale and charset being used by the underlying operating system.\n\nThe [`StandardCharsets`](../../../java/nio/charset/StandardCharsets.html \"class in java.nio.charset\") class defines constants for each of the\nstandard charsets.\n\nTerminology\n-----------\n\nThe name of this class is taken from the terms used in\n[*RFC 2278*](http://www.ietf.org/rfc/rfc2278.txt).\nIn that document a *charset* is defined as the combination of\none or more coded character sets and a character-encoding scheme.\n(This definition is confusing; some other software systems define\n*charset* as a synonym for *coded character set*.)\n\nA *coded character set* is a mapping between a set of abstract\ncharacters and a set of integers. US-ASCII, ISO 8859-1,\nJIS X 0201, and Unicode are examples of coded character sets.\n\nSome standards have defined a *character set* to be simply a\nset of abstract characters without an associated assigned numbering.\nAn alphabet is an example of such a character set. However, the subtle\ndistinction between *character set* and *coded character set*\nis rarely used in practice; the former has become a short form for the\nlatter, including in the Java API specification.\n\nA *character-encoding scheme* is a mapping between one or more\ncoded character sets and a set of octet (eight-bit byte) sequences.\nUTF-8, UTF-16, ISO 2022, and EUC are examples of\ncharacter-encoding schemes. Encoding schemes are often associated with\na particular coded character set; UTF-8, for example, is used only to\nencode Unicode. Some schemes, however, are associated with multiple\ncoded character sets; EUC, for example, can be used to encode\ncharacters in a variety of Asian coded character sets.\n\nWhen a coded character set is used exclusively with a single\ncharacter-encoding scheme then the corresponding charset is usually\nnamed for the coded character set; otherwise a charset is usually named\nfor the encoding scheme and, possibly, the locale of the coded\ncharacter sets that it supports. Hence `US-ASCII` is both the\nname of a coded character set and of the charset that encodes it, while\n`EUC-JP` is the name of the charset that encodes the\nJIS X 0201, JIS X 0208, and JIS X 0212\ncoded character sets for the Japanese language.\n\nThe native character encoding of the Java programming language is\nUTF-16. A charset in the Java platform therefore defines a mapping\nbetween sequences of sixteen-bit UTF-16 code units (that is, sequences\nof chars) and sequences of bytes.";
    const WEIRD_TABLES: &str = "\t\t\t<table class=\"borderless\">\n<caption style=\"display:none\">Regular expression constructs, and what they match</caption>\n<thead style=\"text-align:left\">\n<tr>\n<th id=\"construct\">Construct</th>\n<th id=\"matches\">Matches</th>\n</tr>\n</thead>\n<tbody style=\"text-align:left\">\n\n<tr><th colspan=\"2\" style=\"padding-top:20px\" id=\"characters\">Characters</th></tr>\n\n<tr><th style=\"vertical-align:top; font-weight: normal\" id=\"x\"><i>x</i></th>\n    <td headers=\"matches characters x\">The character <i>x</i></td></tr>\n\n<tr><th style=\"vertical-align:top; font-weight: normal\" id=\"backslash\"><code>nn</code></th>\n    <td headers=\"matches characters backslash\">The backslash character</td></tr>\n    <td headers=\"matches characters ctrl_x\">The control character corresponding to <i>x</i></td></tr>\n\n<tr><th colspan=\"2\" style=\"padding-top:20px\" id=\"classes\">Character classes</th></tr>\n\n<tr><th style=\"vertical-align:top; font-weight:normal\" id=\"simple\"><code>[abc]</code></th>\n    <td headers=\"matches classes simple\"><code>a</code>, <code>b</code>, or <code>c</code> (simple class)</td></tr>\n<tr><th style=\"vertical-align:top; font-weight:normal\" id=\"negation\"><code>[^abc]</code></th>\n    <td headers=\"matches classes negation\">Any character except <code>a</code>, <code>b</code>, or <code>c</code> (negation)</td></tr>\n<tr><th style=\"vertical-align:top; font-weight:normal\" id=\"range\"><code>[a-zA-Z]</code></th>\n    <td headers=\"matches classes range\"><code>a</code> through <code>z</code>\n        or <code>A</code> through <code>Z</code>, inclusive (range)</td></tr>\n<tr><th style=\"vertical-align:top; font-weight:normal\" id=\"subtraction2\"><code>[a-z&amp;&amp;[^m-p]]</code></th>\n    <td headers=\"matches classes subtraction2\"><code>a</code> through <code>z</code>,\n         and not <code>m</code> through <code>p</code>: <code>[a-lq-z]</code>(subtraction)</td></tr>\n\n<tr><th colspan=\"2\" style=\"padding-top:20px\" id=\"predef\">Predefined character classes</th></tr>\n\n<tr><th style=\"vertical-align:top; font-weight:normal\" id=\"any\"><code>.</code></th>\n    <td headers=\"matches predef any\">Any character (may or may not match <a href=\"#lt\">line terminators</a>)</td></tr>\n\n\n<tr><th colspan=\"2\" style=\"padding-top:20px\" id=\"java\">java.lang.Character classes (simple <a href=\"#jcc\">java character type</a>)</th></tr>\n\n<tr><th style=\"vertical-align:top; font-weight:normal\" id=\"javaMirrored\"><code>p{javaMirrored}</code></th>\n    <td headers=\"matches java javaMirrored\">Equivalent to java.lang.Character.isMirrored()</td></tr>\n\n<tr><th colspan=\"2\" style=\"padding-top:20px\" id=\"unicode\">Classes for Unicode scripts, blocks, categories and binary properties</th></tr>\n\n<tr><th style=\"vertical-align:top; font-weight:normal\" id=\"not_uppercase\"><code>[p{L}&amp;&amp;[^p{Lu}]]</code></th>\n    <td headers=\"matches unicode not_uppercase\">Any letter except an\n\n\tuppercase letter (subtraction)</td></tr>\n\n<tr><th colspan=\"2\" style=\"padding-top:20px\" id=\"bounds\">Boundary matchers</th></tr>\n\n<tr><th style=\"vertical-align:top; font-weight:normal\" id=\"begin_line\"><code>^</code></th>\n    <td headers=\"matches bounds begin_line\">The beginning of a line</td></tr>\n<tr><th style=\"vertical-align:top; font-weight:normal\" id=\"end_line\"><code>$</code></th>\n    <td headers=\"matches bounds end_line\">The end of a line</td></tr>\n    <td headers=\"matches bounds end_input_except_term\">The end of the input but for the final\n        <a href=\"#lt\">terminator</a>, if&nbsp;any</td></tr>\n\t\t\t<tr><th style=\"vertical-align:top; font-weight:normal\" id=\"end_input\"><code>\\z</code></th>\n    <td headers=\"matches bounds end_input\">The end of the input</td></tr>\n\n<tr><th colspan=\"2\" style=\"padding-top:20px\" id=\"grapheme\">Unicode Extended Grapheme matcher</th></tr>\n\n<tr><th style=\"vertical-align:top; font-weight:normal\" id=\"grapheme_any\"><code>X</code></th>\n    <td headers=\"matches grapheme grapheme_any\">Any Unicode extended grapheme cluster</td></tr>\n\n</tbody>\n</table>\n";
    const WEIRD_TABLES_MD: &str = "|                         Construct                         |                              Matches                              |\n|-----------------------------------------------------------|-------------------------------------------------------------------|\n| **Characters**                                                                                                               ||\n| ***x***                                                   | The character *x*                                                 |\n| **`nn`**                                                  | The backslash character                                           |\n| The control character corresponding to *x*                |\n| **Character classes**                                                                                                        ||\n| **`[abc]`**                                               | `a`, `b`, or `c` (simple class)                                   |\n| **`[^abc]`**                                              | Any character except `a`, `b`, or `c` (negation)                  |\n| **`[a-zA-Z]`**                                            | `a` through `z` or `A` through `Z`, inclusive (range)             |\n| **`[a-z&&[^m-p]]`**                                       | `a` through `z`, and not `m` through `p`: `[a-lq-z]`(subtraction) |\n| **Predefined character classes**                                                                                             ||\n| **`.`**                                                   | Any character (may or may not match line terminators)             |\n| **java.lang.Character classes (simple java character type)**                                                                 ||\n| **`p{javaMirrored}`**                                     | Equivalent to java.lang.Character.isMirrored()                    |\n| **Classes for Unicode scripts, blocks, categories and binary properties**                                                    ||\n| **`[p{L}&&[^p{Lu}]]`**                                    | Any letter except an uppercase letter (subtraction)               |\n| **Boundary matchers**                                                                                                        ||\n| **`^`**                                                   | The beginning of a line                                           |\n| **`$`**                                                   | The end of a line                                                 |\n| The end of the input but for the final terminator, if any |\n| **`\\z`**                                                  | The end of the input                                              |\n| **Unicode Extended Grapheme matcher**                                                                                        ||\n| **`X`**                                                   | Any Unicode extended grapheme cluster                             |\n[Regular expression constructs, and what they match]";
    const WEIRD_LI_HTML: &str = "<div>\n<p> Perl constructs not supported by this class: </p>\n\n <ul>\n    <li><p> Predefined character classes (Unicode character)\n\t\t\t    <p><tt>\\X&nbsp;&nbsp;&nbsp;&nbsp;</tt>Match Unicode\n    <a href=\"http://www.unicode.org/reports/tr18/#Default_Grapheme_Clusters\">\n    <i>extended grapheme cluster</i></a>\n    </p></li>\n\n    <li><p> The backreference constructs, <tt>\\g{</tt><i>n</i><tt>}</tt> for\n    the <i>n</i><sup>th</sup><a href=\"#cg\">capturing group</a> and\n\t\t\t    <tt>\\g{</tt><i>name</i><tt>}</tt> for\n    <a href=\"#groupname\">named-capturing group</a>.\n    </p></li>\n\n    <li><p> The named character construct, <tt>\\N{</tt><i>name</i><tt>}</tt>\n    for a Unicode character by its name.\n    </p></li>\n\n    <li><p> The conditional constructs\n    <tt>(?(</tt><i>condition</i><tt>)</tt><i>X</i><tt>)</tt> and\n    <tt>(?(</tt><i>condition</i><tt>)</tt><i>X</i><tt>|</tt><i>Y</i><tt>)</tt>,\n    </p></li>\n\n    <li><p> The embedded code constructs <tt>(?{</tt><i>code</i><tt>})</tt>\n    and <tt>(??{</tt><i>code</i><tt>})</tt>,</p></li>\n\n    <li><p> The embedded comment syntax <tt>(?#comment)</tt>, and </p></li>\n\n\t\t\t    <li><p> The preprocessing operations <tt>\\l</tt> <tt>&#92;u</tt>,\n\t\t\t    <tt>\\L</tt>, and <tt>\\U</tt>.  </p></li>\n\n </ul>\n\n <p> Constructs supported by this class but not by Perl: </p>\n</div>";
    const WEIRD_LI_MD: &str = "Perl constructs not supported by this class:\n\n* Predefined character classes (Unicode character)\n\n  `\\X `Match Unicode\n  [*extended grapheme cluster*](http://www.unicode.org/reports/tr18/#Default_Grapheme_Clusters)\n* The backreference constructs, `\\g{`*n* `}` for\n  the *n* ^th^capturing group and\n  `\\g{`*name* `}` for\n  named-capturing group.\n\n* The named character construct, `\\N{`*name* `}`\n  for a Unicode character by its name.\n\n* The conditional constructs\n  `(?(`*condition* `)`*X* `)` and\n  `(?(`*condition* `)`*X* `|`*Y* `)`,\n\n* The embedded code constructs `(?{`*code* `})`\n  and `(??{`*code* `})`,\n\n* The embedded comment syntax `(?#comment)`, and\n\n* The preprocessing operations `\\l` `\\u`,\n  `\\L`, and `\\U`.\n\nConstructs supported by this class but not by Perl:";
    const TABLES_BLOCKQUOTE_JAVADOC: &str = "<p> Character classes may appear within other character classes, and\n  may be composed by the union operator (implicit) and the intersection\n  operator (<tt>&amp;&amp;</tt>).\n  The union operator denotes a class that contains every character that is\n  in at least one of its operand classes.  The intersection operator\n  denotes a class that contains every character that is in both of its\n  operand classes.\n\n  <p> The precedence of character-class operators is as follows, from\n  highest to lowest:\n\n  <blockquote><table border=\"0\" cellpadding=\"1\" cellspacing=\"0\"\n               summary=\"Precedence of character class operators.\">\n    <tr><th>1&nbsp;&nbsp;&nbsp;&nbsp;</th>\n      <td>Literal escape&nbsp;&nbsp;&nbsp;&nbsp;</td>\n      <td><tt>\\x</tt></td></tr>\n   <tr><th>2&nbsp;&nbsp;&nbsp;&nbsp;</th>\n      <td>Grouping</td>\n      <td><tt>[...]</tt></td></tr>\n   <tr><th>3&nbsp;&nbsp;&nbsp;&nbsp;</th>\n      <td>Range</td>\n      <td><tt>a-z</tt></td></tr>\n    <tr><th>4&nbsp;&nbsp;&nbsp;&nbsp;</th>\n      <td>Union</td>\n      <td><tt>[a-e][i-u]</tt></td></tr>\n    <tr><th>5&nbsp;&nbsp;&nbsp;&nbsp;</th>\n      <td>Intersection</td>\n      <td>{@code [a-z&&[aeiou]]}</td></tr>\n  </table></blockquote>\n\n  <p> Note that a different set of metacharacters are in effect inside\n  a character class than outside a character class. For instance, the\n  regular expression <tt>.</tt> loses its special meaning inside a\n  character class, while the expression <tt>-</tt> becomes a range\n  forming metacharacter.";
    const TABLES_BLOCKQUOTE_MD: &str = "Character classes may appear within other character classes, and\nmay be composed by the union operator (implicit) and the intersection\noperator (`&&`).\nThe union operator denotes a class that contains every character that is\nin at least one of its operand classes. The intersection operator\ndenotes a class that contains every character that is in both of its\noperand classes.\n\nThe precedence of character-class operators is as follows, from\nhighest to lowest:\n\n> |       |                |                  |\n> |-------|----------------|------------------|\n> | **1** | Literal escape | `\\x`             |\n> | **2** | Grouping       | `[...]`          |\n> | **3** | Range          | `a-z`            |\n> | **4** | Union          | `[a-e][i-u]`     |\n> | **5** | Intersection   | `[a-z&&[aeiou]]` |\n\nNote that a different set of metacharacters are in effect inside\na character class than outside a character class. For instance, the\nregular expression `.` loses its special meaning inside a\ncharacter class, while the expression `-` becomes a range\nforming metacharacter.";

    #[test]
    fn test_boundaries() {
        assert!(javadoc_to_markdown(Some("")).unwrap().is_empty());
        assert_eq!(None, javadoc_to_markdown(None));
        assert_eq!(None, javadoc_to_markdown(None));
    }

    #[test]
    fn test_get_as_string() {
        let result = md(RAW_JAVADOC_0);
        assert_eq!(independent(MARKDOWN_0), independent(&result));
    }

    #[test]
    fn test_markdown_table_no_thead() {
        let result = md(RAW_JAVADOC_TABLE_0);
        assert_eq!(independent(MARKDOWN_TABLE_0), independent(&result));
    }

    #[test]
    fn test_markdown_table_insert_blank_header() {
        let result = md(RAW_JAVADOC_TABLE_1);
        assert_eq!(independent(MARKDOWN_TABLE_1), independent(&result));
    }

    #[test]
    fn test_link_to_file_is_present() {
        let converted = md(RAW_JAVADOC_HTML_1);
        let (label, uri) = extract_label_and_uri_from_link_markdown(&converted);
        assert_eq!("File", label);
        assert_eq!("file://some_location", uri);
    }

    #[test]
    fn test_link_to_jdt_file_is_present() {
        let converted = md(RAW_JAVADOC_HTML_2);
        let (label, uri) = extract_label_and_uri_from_link_markdown(&converted);
        assert_eq!("JDT", label);
        assert_eq!("jdt://some_location", uri);
    }

    /// Ensures the custom anchor renderer encodes parentheses in jdt:// hrefs so
    /// Markdown link syntax [text](url) is not broken. See #3705.
    #[test]
    fn test_jdt_link_with_parentheses_in_url_is_encoded_by_anchor_renderer() {
        let html = "<a href=\"jdt://contents/foo.jar/pkg/Clazz.class?=x/%3Cpkg(Clazz.class#42\">waitForExit</a>";
        let converted = md(html);
        let (label, uri) = extract_label_and_uri_from_link_markdown(&converted);
        assert_eq!("waitForExit", label);
        assert!(uri.contains("%28Clazz.class#42"), "jdt URL should contain encoded opening parenthesis: {}", uri);
        assert!(!uri.contains("(Clazz.class"), "jdt URL should not contain raw ( before .class: {}", uri);
    }

    #[test]
    fn test_see_tag() {
        let converted = md(RAW_JAVADOC_HTML_SEE);
        assert_eq!(
            "* **See Also:**\n  * [Online docs for java](https://docs.oracle.com/javase/7/docs/api/)",
            crate::javadoc::converter::dos2unix(&converted)
        );
    }

    #[test]
    fn test_param_tag() {
        let converted = md(RAW_JAVADOC_HTML_PARAM);
        assert_eq!("* **Parameters:**\n  * **someString** the string to enter", crate::javadoc::converter::dos2unix(&converted));
    }

    #[test]
    fn test_since_tag() {
        let converted = md(RAW_JAVADOC_HTML_SINCE);
        assert_eq!("* **Since:**\n  * 0.0.1", crate::javadoc::converter::dos2unix(&converted));
    }

    #[test]
    fn test_version_tag() {
        let converted = md(RAW_JAVADOC_HTML_VERSION);
        assert_eq!("* @version\n  * 0.0.1", crate::javadoc::converter::dos2unix(&converted));
    }

    #[test]
    fn test_throws_tag() {
        let converted = md(RAW_JAVADOC_HTML_THROWS);
        assert_eq!("* **Throws:**\n  * IOException", crate::javadoc::converter::dos2unix(&converted));
    }

    #[test]
    fn test_author_tag() {
        let converted = md(RAW_JAVADOC_HTML_AUTHOR);
        let expected = "* **Author:**\n  * author one\n  * author two\n  * author three";
        assert_eq!(expected, crate::javadoc::converter::dos2unix(&converted));
    }

    #[test]
    fn test_mixed_tag() {
        let converted = md(RAW_JAVADOC_HTML_MIX);
        let expected = "A super important method.\n\n* **See Also:**\n  * java.lang.String.split(String, int)\n  * java.lang.String.split(String)\n* **Author:**\n  * JSR-666 Expert Group\n  * Chuck Norris\n* **Since:**\n  * 1.666\n* @spec\n  * JSR-666";
        assert_eq!(expected, crate::javadoc::converter::dos2unix(&converted));
    }

    #[test]
    fn test_code_tag() {
        let javadoc = "This is a method that does something.\n<pre>\nint x = 10;\nSystem.out.println(x);\n</pre>\n";
        let converted = md(javadoc);
        assert_eq!("This is a method that does something.\n\n```\nint x = 10;\nSystem.out.println(x);\n```", converted);
    }

    #[test]
    fn test_complex_javadoc() {
        let converted = md(CHARSET_HTML_JAVADOC);
        assert_eq!(CHARSET_MD_JAVADOC, converted);
    }

    #[test]
    fn test_weird_tables_javadoc() {
        let converted = md(WEIRD_TABLES);
        assert_eq!(WEIRD_TABLES_MD, converted);
    }

    #[test]
    fn test_weird_li_javadoc() {
        let converted = md(WEIRD_LI_HTML);
        assert_eq!(WEIRD_LI_MD, converted);
    }

    #[test]
    fn test_tables_blockquote_javadoc() {
        let converted = md(TABLES_BLOCKQUOTE_JAVADOC);
        assert_eq!(TABLES_BLOCKQUOTE_MD, converted);
    }
}

#[cfg(test)]
mod java_doc2_plain_text_converter_test {
    //! Port of upstream `JavaDoc2PlainTextConverterTest`.
    use super::abstract_javadoc_converter_test::*;
    use super::javadoc_to_plain_text;

    const PLAINTEXT_0: &str = "This Javadoc contains some  code , a link to IOException and a table \nheader 1 header 2 \ndata 1 data 2 \n literally <b>literal</b> and now a list: \n * Coffee \n   - Mocha \n   - Latte \n * Tea \n   - Darjeeling \n   - Early Grey \n * Parameters:\n   - param1 the first parameter\n   - param2 the 2nd parameter\n   - param3\n * Returns:\n   - some kind of result\n * Throws:\n   - NastyException a nasty exception\n   - IOException another nasty exception\n * Author:\n   - Ralf <mailto:foo@bar.com>\n   - Andrew <mailto:bar@foo.com>\n * Since:\n   - 1.0\n   - 0\n * @unknown\n   - unknown tag\n * @unknown\n   - another unknown tag";

    fn independent(s: &str) -> String {
        s.replace("\r\n", "\n").replace('\r', "\n")
    }

    #[test]
    fn test_boundaries() {
        assert!(javadoc_to_plain_text(Some("")).unwrap().is_empty());
        assert_eq!(None, javadoc_to_plain_text(None));
        assert_eq!(None, javadoc_to_plain_text(None));
    }

    #[test]
    fn test_get_as_string() {
        let result = javadoc_to_plain_text(Some(RAW_JAVADOC_0)).unwrap();
        assert_eq!(independent(PLAINTEXT_0), independent(&result));
    }
}

/// `ResourceUtils.dos2Unix`
#[cfg(test)]
pub(crate) fn dos2unix(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// Differential check against the real jdt.ls converters: set `JAVADOC_CORPUS` to a
/// directory with `in/`, `md/` and `txt/` (oracle outputs) and run with `--ignored`.
#[cfg(test)]
mod corpus_diff {
    #[test]
    #[ignore = "differential check against oracle outputs; needs JAVADOC_CORPUS"]
    fn corpus() {
        let dir = match std::env::var("JAVADOC_CORPUS") {
            Ok(d) => std::path::PathBuf::from(d),
            Err(_) => return,
        };
        let mut names: Vec<_> = std::fs::read_dir(dir.join("in")).unwrap().map(|e| e.unwrap().file_name()).collect();
        names.sort();
        let (mut ok_md, mut ok_txt, mut total) = (0, 0, 0);
        let mut bad = Vec::new();
        let _ = std::fs::remove_dir_all(dir.join("rust_md"));
        let _ = std::fs::remove_dir_all(dir.join("rust_txt"));
        for n in names {
            let input = std::fs::read_to_string(dir.join("in").join(&n)).unwrap();
            let exp_md = std::fs::read_to_string(dir.join("md").join(&n)).unwrap();
            let exp_txt = std::fs::read_to_string(dir.join("txt").join(&n)).unwrap();
            let md = std::panic::catch_unwind(|| super::javadoc_to_markdown(Some(&input)).unwrap())
                .unwrap_or_else(|_| "!!PANIC".into());
            let txt = std::panic::catch_unwind(|| super::javadoc_to_plain_text(Some(&input)).unwrap())
                .unwrap_or_else(|_| "!!PANIC".into());
            total += 1;
            if md == exp_md {
                ok_md += 1;
            } else {
                bad.push(format!("md {}", n.to_string_lossy()));
                let _ = std::fs::create_dir_all(dir.join("rust_md"));
                std::fs::write(dir.join("rust_md").join(&n), &md).unwrap();
            }
            if txt == exp_txt {
                ok_txt += 1;
            } else {
                bad.push(format!("txt {}", n.to_string_lossy()));
                let _ = std::fs::create_dir_all(dir.join("rust_txt"));
                std::fs::write(dir.join("rust_txt").join(&n), &txt).unwrap();
            }
        }
        std::fs::write(dir.join("mismatches.txt"), bad.join("\n")).unwrap();
        eprintln!("corpus: {total} inputs, markdown {ok_md} match, plain text {ok_txt} match");
    }
}
