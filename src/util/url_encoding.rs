use percent_encoding::{utf8_percent_encode, AsciiSet, PercentEncode, CONTROLS};

// I _think_ I want the equivalent of JS encodeURIComponent().
// But, I'm following what the spec says:
// https://url.spec.whatwg.org/#component-percent-encode-set
// ...and it's actually significantly more hardcore than what MDN says:
// https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/encodeURI
const URI_COMPONENT: AsciiSet = CONTROLS
    // Component set is these...
    .add(b'$')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b',')
    // ...plus the userinfo set, which is these...
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'=')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'|')
    // ...plus the path set, which is these...
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    // ...plus the query set, which is these plus the control set:
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'<')
    .add(b'>');

pub fn encode_uri_component(c: &str) -> PercentEncode {
    utf8_percent_encode(c, &URI_COMPONENT)
}
