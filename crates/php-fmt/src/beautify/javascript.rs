//! js-beautify's JavaScript beautifier (`js/src/javascript`), used for
//! `<script>` contents of templates. Includes Intelephense's change: a word
//! `<?php N ?>` changes the indentation by `N` levels.
//!
//! js-beautify is MIT licensed, Copyright (c) 2007-2018 Einar Lielmanis,
//! Liam Newman, and contributors. The identifier patterns come from acorn
//! (MIT, Marijn Haverbeke).

// the branches mirror js-beautify's code one to one
#![allow(clippy::if_same_then_else, clippy::collapsible_if)]

use super::core::{
    BaseOptions, Directives, InputScanner, Output, Pattern, Re, TemplatablePattern, WhitespacePattern,
};
use super::html::ChildOptions;

// Generated from js-beautify's acorn.js (identifier patterns).
const IDENTIFIER: &str = r"(?:\\u[0-9a-fA-F]{4}|\\u\{[0-9a-fA-F]+\}|[\x23\x24\x40\x41-\x5a\x5f\x61-\x7a\xaa\xb5\xba\xc0-\xd6\xd8-\xf6\xf8-\u02c1\u02c6-\u02d1\u02e0-\u02e4\u02ec\u02ee\u0370-\u0374\u0376\u0377\u037a-\u037d\u0386\u0388-\u038a\u038c\u038e-\u03a1\u03a3-\u03f5\u03f7-\u0481\u048a-\u0527\u0531-\u0556\u0559\u0561-\u0587\u05d0-\u05ea\u05f0-\u05f2\u0620-\u064a\u066e\u066f\u0671-\u06d3\u06d5\u06e5\u06e6\u06ee\u06ef\u06fa-\u06fc\u06ff\u0710\u0712-\u072f\u074d-\u07a5\u07b1\u07ca-\u07ea\u07f4\u07f5\u07fa\u0800-\u0815\u081a\u0824\u0828\u0840-\u0858\u08a0\u08a2-\u08ac\u0904-\u0939\u093d\u0950\u0958-\u0961\u0971-\u0977\u0979-\u097f\u0985-\u098c\u098f\u0990\u0993-\u09a8\u09aa-\u09b0\u09b2\u09b6-\u09b9\u09bd\u09ce\u09dc\u09dd\u09df-\u09e1\u09f0\u09f1\u0a05-\u0a0a\u0a0f\u0a10\u0a13-\u0a28\u0a2a-\u0a30\u0a32\u0a33\u0a35\u0a36\u0a38\u0a39\u0a59-\u0a5c\u0a5e\u0a72-\u0a74\u0a85-\u0a8d\u0a8f-\u0a91\u0a93-\u0aa8\u0aaa-\u0ab0\u0ab2\u0ab3\u0ab5-\u0ab9\u0abd\u0ad0\u0ae0\u0ae1\u0b05-\u0b0c\u0b0f\u0b10\u0b13-\u0b28\u0b2a-\u0b30\u0b32\u0b33\u0b35-\u0b39\u0b3d\u0b5c\u0b5d\u0b5f-\u0b61\u0b71\u0b83\u0b85-\u0b8a\u0b8e-\u0b90\u0b92-\u0b95\u0b99\u0b9a\u0b9c\u0b9e\u0b9f\u0ba3\u0ba4\u0ba8-\u0baa\u0bae-\u0bb9\u0bd0\u0c05-\u0c0c\u0c0e-\u0c10\u0c12-\u0c28\u0c2a-\u0c33\u0c35-\u0c39\u0c3d\u0c58\u0c59\u0c60\u0c61\u0c85-\u0c8c\u0c8e-\u0c90\u0c92-\u0ca8\u0caa-\u0cb3\u0cb5-\u0cb9\u0cbd\u0cde\u0ce0\u0ce1\u0cf1\u0cf2\u0d05-\u0d0c\u0d0e-\u0d10\u0d12-\u0d3a\u0d3d\u0d4e\u0d60\u0d61\u0d7a-\u0d7f\u0d85-\u0d96\u0d9a-\u0db1\u0db3-\u0dbb\u0dbd\u0dc0-\u0dc6\u0e01-\u0e30\u0e32\u0e33\u0e40-\u0e46\u0e81\u0e82\u0e84\u0e87\u0e88\u0e8a\u0e8d\u0e94-\u0e97\u0e99-\u0e9f\u0ea1-\u0ea3\u0ea5\u0ea7\u0eaa\u0eab\u0ead-\u0eb0\u0eb2\u0eb3\u0ebd\u0ec0-\u0ec4\u0ec6\u0edc-\u0edf\u0f00\u0f40-\u0f47\u0f49-\u0f6c\u0f88-\u0f8c\u1000-\u102a\u103f\u1050-\u1055\u105a-\u105d\u1061\u1065\u1066\u106e-\u1070\u1075-\u1081\u108e\u10a0-\u10c5\u10c7\u10cd\u10d0-\u10fa\u10fc-\u1248\u124a-\u124d\u1250-\u1256\u1258\u125a-\u125d\u1260-\u1288\u128a-\u128d\u1290-\u12b0\u12b2-\u12b5\u12b8-\u12be\u12c0\u12c2-\u12c5\u12c8-\u12d6\u12d8-\u1310\u1312-\u1315\u1318-\u135a\u1380-\u138f\u13a0-\u13f4\u1401-\u166c\u166f-\u167f\u1681-\u169a\u16a0-\u16ea\u16ee-\u16f0\u1700-\u170c\u170e-\u1711\u1720-\u1731\u1740-\u1751\u1760-\u176c\u176e-\u1770\u1780-\u17b3\u17d7\u17dc\u1820-\u1877\u1880-\u18a8\u18aa\u18b0-\u18f5\u1900-\u191c\u1950-\u196d\u1970-\u1974\u1980-\u19ab\u19c1-\u19c7\u1a00-\u1a16\u1a20-\u1a54\u1aa7\u1b05-\u1b33\u1b45-\u1b4b\u1b83-\u1ba0\u1bae\u1baf\u1bba-\u1be5\u1c00-\u1c23\u1c4d-\u1c4f\u1c5a-\u1c7d\u1ce9-\u1cec\u1cee-\u1cf1\u1cf5\u1cf6\u1d00-\u1dbf\u1e00-\u1f15\u1f18-\u1f1d\u1f20-\u1f45\u1f48-\u1f4d\u1f50-\u1f57\u1f59\u1f5b\u1f5d\u1f5f-\u1f7d\u1f80-\u1fb4\u1fb6-\u1fbc\u1fbe\u1fc2-\u1fc4\u1fc6-\u1fcc\u1fd0-\u1fd3\u1fd6-\u1fdb\u1fe0-\u1fec\u1ff2-\u1ff4\u1ff6-\u1ffc\u2071\u207f\u2090-\u209c\u2102\u2107\u210a-\u2113\u2115\u2119-\u211d\u2124\u2126\u2128\u212a-\u212d\u212f-\u2139\u213c-\u213f\u2145-\u2149\u214e\u2160-\u2188\u2c00-\u2c2e\u2c30-\u2c5e\u2c60-\u2ce4\u2ceb-\u2cee\u2cf2\u2cf3\u2d00-\u2d25\u2d27\u2d2d\u2d30-\u2d67\u2d6f\u2d80-\u2d96\u2da0-\u2da6\u2da8-\u2dae\u2db0-\u2db6\u2db8-\u2dbe\u2dc0-\u2dc6\u2dc8-\u2dce\u2dd0-\u2dd6\u2dd8-\u2dde\u2e2f\u3005-\u3007\u3021-\u3029\u3031-\u3035\u3038-\u303c\u3041-\u3096\u309d-\u309f\u30a1-\u30fa\u30fc-\u30ff\u3105-\u312d\u3131-\u318e\u31a0-\u31ba\u31f0-\u31ff\u3400-\u4db5\u4e00-\u9fcc\ua000-\ua48c\ua4d0-\ua4fd\ua500-\ua60c\ua610-\ua61f\ua62a\ua62b\ua640-\ua66e\ua67f-\ua697\ua6a0-\ua6ef\ua717-\ua71f\ua722-\ua788\ua78b-\ua78e\ua790-\ua793\ua7a0-\ua7aa\ua7f8-\ua801\ua803-\ua805\ua807-\ua80a\ua80c-\ua822\ua840-\ua873\ua882-\ua8b3\ua8f2-\ua8f7\ua8fb\ua90a-\ua925\ua930-\ua946\ua960-\ua97c\ua984-\ua9b2\ua9cf\uaa00-\uaa28\uaa40-\uaa42\uaa44-\uaa4b\uaa60-\uaa76\uaa7a\uaa80-\uaaaf\uaab1\uaab5\uaab6\uaab9-\uaabd\uaac0\uaac2\uaadb-\uaadd\uaae0-\uaaea\uaaf2-\uaaf4\uab01-\uab06\uab09-\uab0e\uab11-\uab16\uab20-\uab26\uab28-\uab2e\uabc0-\uabe2\uac00-\ud7a3\ud7b0-\ud7c6\ud7cb-\ud7fb\uf900-\ufa6d\ufa70-\ufad9\ufb00-\ufb06\ufb13-\ufb17\ufb1d\ufb1f-\ufb28\ufb2a-\ufb36\ufb38-\ufb3c\ufb3e\ufb40\ufb41\ufb43\ufb44\ufb46-\ufbb1\ufbd3-\ufd3d\ufd50-\ufd8f\ufd92-\ufdc7\ufdf0-\ufdfb\ufe70-\ufe74\ufe76-\ufefc\uff21-\uff3a\uff41-\uff5a\uff66-\uffbe\uffc2-\uffc7\uffca-\uffcf\uffd2-\uffd7\uffda-\uffdc])(?:\\u[0-9a-fA-F]{4}|\\u\{[0-9a-fA-F]+\}|[\x24\x30-\x39\x41-\x5a\x5f\x61-\x7a\xaa\xb5\xba\xc0-\xd6\xd8-\xf6\xf8-\u02c1\u02c6-\u02d1\u02e0-\u02e4\u02ec\u02ee\u0370-\u0374\u0376\u0377\u037a-\u037d\u0386\u0388-\u038a\u038c\u038e-\u03a1\u03a3-\u03f5\u03f7-\u0481\u048a-\u0527\u0531-\u0556\u0559\u0561-\u0587\u05d0-\u05ea\u05f0-\u05f2\u0620-\u064a\u066e\u066f\u0671-\u06d3\u06d5\u06e5\u06e6\u06ee\u06ef\u06fa-\u06fc\u06ff\u0710\u0712-\u072f\u074d-\u07a5\u07b1\u07ca-\u07ea\u07f4\u07f5\u07fa\u0800-\u0815\u081a\u0824\u0828\u0840-\u0858\u08a0\u08a2-\u08ac\u0904-\u0939\u093d\u0950\u0958-\u0961\u0971-\u0977\u0979-\u097f\u0985-\u098c\u098f\u0990\u0993-\u09a8\u09aa-\u09b0\u09b2\u09b6-\u09b9\u09bd\u09ce\u09dc\u09dd\u09df-\u09e1\u09f0\u09f1\u0a05-\u0a0a\u0a0f\u0a10\u0a13-\u0a28\u0a2a-\u0a30\u0a32\u0a33\u0a35\u0a36\u0a38\u0a39\u0a59-\u0a5c\u0a5e\u0a72-\u0a74\u0a85-\u0a8d\u0a8f-\u0a91\u0a93-\u0aa8\u0aaa-\u0ab0\u0ab2\u0ab3\u0ab5-\u0ab9\u0abd\u0ad0\u0ae0\u0ae1\u0b05-\u0b0c\u0b0f\u0b10\u0b13-\u0b28\u0b2a-\u0b30\u0b32\u0b33\u0b35-\u0b39\u0b3d\u0b5c\u0b5d\u0b5f-\u0b61\u0b71\u0b83\u0b85-\u0b8a\u0b8e-\u0b90\u0b92-\u0b95\u0b99\u0b9a\u0b9c\u0b9e\u0b9f\u0ba3\u0ba4\u0ba8-\u0baa\u0bae-\u0bb9\u0bd0\u0c05-\u0c0c\u0c0e-\u0c10\u0c12-\u0c28\u0c2a-\u0c33\u0c35-\u0c39\u0c3d\u0c58\u0c59\u0c60\u0c61\u0c85-\u0c8c\u0c8e-\u0c90\u0c92-\u0ca8\u0caa-\u0cb3\u0cb5-\u0cb9\u0cbd\u0cde\u0ce0\u0ce1\u0cf1\u0cf2\u0d05-\u0d0c\u0d0e-\u0d10\u0d12-\u0d3a\u0d3d\u0d4e\u0d60\u0d61\u0d7a-\u0d7f\u0d85-\u0d96\u0d9a-\u0db1\u0db3-\u0dbb\u0dbd\u0dc0-\u0dc6\u0e01-\u0e30\u0e32\u0e33\u0e40-\u0e46\u0e81\u0e82\u0e84\u0e87\u0e88\u0e8a\u0e8d\u0e94-\u0e97\u0e99-\u0e9f\u0ea1-\u0ea3\u0ea5\u0ea7\u0eaa\u0eab\u0ead-\u0eb0\u0eb2\u0eb3\u0ebd\u0ec0-\u0ec4\u0ec6\u0edc-\u0edf\u0f00\u0f40-\u0f47\u0f49-\u0f6c\u0f88-\u0f8c\u1000-\u102a\u103f\u1050-\u1055\u105a-\u105d\u1061\u1065\u1066\u106e-\u1070\u1075-\u1081\u108e\u10a0-\u10c5\u10c7\u10cd\u10d0-\u10fa\u10fc-\u1248\u124a-\u124d\u1250-\u1256\u1258\u125a-\u125d\u1260-\u1288\u128a-\u128d\u1290-\u12b0\u12b2-\u12b5\u12b8-\u12be\u12c0\u12c2-\u12c5\u12c8-\u12d6\u12d8-\u1310\u1312-\u1315\u1318-\u135a\u1380-\u138f\u13a0-\u13f4\u1401-\u166c\u166f-\u167f\u1681-\u169a\u16a0-\u16ea\u16ee-\u16f0\u1700-\u170c\u170e-\u1711\u1720-\u1731\u1740-\u1751\u1760-\u176c\u176e-\u1770\u1780-\u17b3\u17d7\u17dc\u1820-\u1877\u1880-\u18a8\u18aa\u18b0-\u18f5\u1900-\u191c\u1950-\u196d\u1970-\u1974\u1980-\u19ab\u19c1-\u19c7\u1a00-\u1a16\u1a20-\u1a54\u1aa7\u1b05-\u1b33\u1b45-\u1b4b\u1b83-\u1ba0\u1bae\u1baf\u1bba-\u1be5\u1c00-\u1c23\u1c4d-\u1c4f\u1c5a-\u1c7d\u1ce9-\u1cec\u1cee-\u1cf1\u1cf5\u1cf6\u1d00-\u1dbf\u1e00-\u1f15\u1f18-\u1f1d\u1f20-\u1f45\u1f48-\u1f4d\u1f50-\u1f57\u1f59\u1f5b\u1f5d\u1f5f-\u1f7d\u1f80-\u1fb4\u1fb6-\u1fbc\u1fbe\u1fc2-\u1fc4\u1fc6-\u1fcc\u1fd0-\u1fd3\u1fd6-\u1fdb\u1fe0-\u1fec\u1ff2-\u1ff4\u1ff6-\u1ffc\u2071\u207f\u2090-\u209c\u2102\u2107\u210a-\u2113\u2115\u2119-\u211d\u2124\u2126\u2128\u212a-\u212d\u212f-\u2139\u213c-\u213f\u2145-\u2149\u214e\u2160-\u2188\u2c00-\u2c2e\u2c30-\u2c5e\u2c60-\u2ce4\u2ceb-\u2cee\u2cf2\u2cf3\u2d00-\u2d25\u2d27\u2d2d\u2d30-\u2d67\u2d6f\u2d80-\u2d96\u2da0-\u2da6\u2da8-\u2dae\u2db0-\u2db6\u2db8-\u2dbe\u2dc0-\u2dc6\u2dc8-\u2dce\u2dd0-\u2dd6\u2dd8-\u2dde\u2e2f\u3005-\u3007\u3021-\u3029\u3031-\u3035\u3038-\u303c\u3041-\u3096\u309d-\u309f\u30a1-\u30fa\u30fc-\u30ff\u3105-\u312d\u3131-\u318e\u31a0-\u31ba\u31f0-\u31ff\u3400-\u4db5\u4e00-\u9fcc\ua000-\ua48c\ua4d0-\ua4fd\ua500-\ua60c\ua610-\ua61f\ua62a\ua62b\ua640-\ua66e\ua67f-\ua697\ua6a0-\ua6ef\ua717-\ua71f\ua722-\ua788\ua78b-\ua78e\ua790-\ua793\ua7a0-\ua7aa\ua7f8-\ua801\ua803-\ua805\ua807-\ua80a\ua80c-\ua822\ua840-\ua873\ua882-\ua8b3\ua8f2-\ua8f7\ua8fb\ua90a-\ua925\ua930-\ua946\ua960-\ua97c\ua984-\ua9b2\ua9cf\uaa00-\uaa28\uaa40-\uaa42\uaa44-\uaa4b\uaa60-\uaa76\uaa7a\uaa80-\uaaaf\uaab1\uaab5\uaab6\uaab9-\uaabd\uaac0\uaac2\uaadb-\uaadd\uaae0-\uaaea\uaaf2-\uaaf4\uab01-\uab06\uab09-\uab0e\uab11-\uab16\uab20-\uab26\uab28-\uab2e\uabc0-\uabe2\uac00-\ud7a3\ud7b0-\ud7c6\ud7cb-\ud7fb\uf900-\ufa6d\ufa70-\ufad9\ufb00-\ufb06\ufb13-\ufb17\ufb1d\ufb1f-\ufb28\ufb2a-\ufb36\ufb38-\ufb3c\ufb3e\ufb40\ufb41\ufb43\ufb44\ufb46-\ufbb1\ufbd3-\ufd3d\ufd50-\ufd8f\ufd92-\ufdc7\ufdf0-\ufdfb\ufe70-\ufe74\ufe76-\ufefc\uff21-\uff3a\uff41-\uff5a\uff66-\uffbe\uffc2-\uffc7\uffca-\uffcf\uffd2-\uffd7\uffda-\uffdc\u0300-\u036f\u0483-\u0487\u0591-\u05bd\u05bf\u05c1\u05c2\u05c4\u05c5\u05c7\u0610-\u061a\u0620-\u0649\u0672-\u06d3\u06e7-\u06e8\u06fb-\u06fc\u0730-\u074a\u0800-\u0814\u081b-\u0823\u0825-\u0827\u0829-\u082d\u0840-\u0857\u08e4-\u08fe\u0900-\u0903\u093a-\u093c\u093e-\u094f\u0951-\u0957\u0962-\u0963\u0966-\u096f\u0981-\u0983\u09bc\u09be-\u09c4\u09c7\u09c8\u09d7\u09df-\u09e0\u0a01-\u0a03\u0a3c\u0a3e-\u0a42\u0a47\u0a48\u0a4b-\u0a4d\u0a51\u0a66-\u0a71\u0a75\u0a81-\u0a83\u0abc\u0abe-\u0ac5\u0ac7-\u0ac9\u0acb-\u0acd\u0ae2-\u0ae3\u0ae6-\u0aef\u0b01-\u0b03\u0b3c\u0b3e-\u0b44\u0b47\u0b48\u0b4b-\u0b4d\u0b56\u0b57\u0b5f-\u0b60\u0b66-\u0b6f\u0b82\u0bbe-\u0bc2\u0bc6-\u0bc8\u0bca-\u0bcd\u0bd7\u0be6-\u0bef\u0c01-\u0c03\u0c46-\u0c48\u0c4a-\u0c4d\u0c55\u0c56\u0c62-\u0c63\u0c66-\u0c6f\u0c82\u0c83\u0cbc\u0cbe-\u0cc4\u0cc6-\u0cc8\u0cca-\u0ccd\u0cd5\u0cd6\u0ce2-\u0ce3\u0ce6-\u0cef\u0d02\u0d03\u0d46-\u0d48\u0d57\u0d62-\u0d63\u0d66-\u0d6f\u0d82\u0d83\u0dca\u0dcf-\u0dd4\u0dd6\u0dd8-\u0ddf\u0df2\u0df3\u0e34-\u0e3a\u0e40-\u0e45\u0e50-\u0e59\u0eb4-\u0eb9\u0ec8-\u0ecd\u0ed0-\u0ed9\u0f18\u0f19\u0f20-\u0f29\u0f35\u0f37\u0f39\u0f41-\u0f47\u0f71-\u0f84\u0f86-\u0f87\u0f8d-\u0f97\u0f99-\u0fbc\u0fc6\u1000-\u1029\u1040-\u1049\u1067-\u106d\u1071-\u1074\u1082-\u108d\u108f-\u109d\u135d-\u135f\u170e-\u1710\u1720-\u1730\u1740-\u1750\u1772\u1773\u1780-\u17b2\u17dd\u17e0-\u17e9\u180b-\u180d\u1810-\u1819\u1920-\u192b\u1930-\u193b\u1951-\u196d\u19b0-\u19c0\u19c8-\u19c9\u19d0-\u19d9\u1a00-\u1a15\u1a20-\u1a53\u1a60-\u1a7c\u1a7f-\u1a89\u1a90-\u1a99\u1b46-\u1b4b\u1b50-\u1b59\u1b6b-\u1b73\u1bb0-\u1bb9\u1be6-\u1bf3\u1c00-\u1c22\u1c40-\u1c49\u1c5b-\u1c7d\u1cd0-\u1cd2\u1d00-\u1dbe\u1e01-\u1f15\u200c\u200d\u203f\u2040\u2054\u20d0-\u20dc\u20e1\u20e5-\u20f0\u2d81-\u2d96\u2de0-\u2dff\u3021-\u3028\u3099\u309a\ua640-\ua66d\ua674-\ua67d\ua69f\ua6f0-\ua6f1\ua7f8-\ua800\ua806\ua80b\ua823-\ua827\ua880-\ua881\ua8b4-\ua8c4\ua8d0-\ua8d9\ua8f3-\ua8f7\ua900-\ua909\ua926-\ua92d\ua930-\ua945\ua980-\ua983\ua9b3-\ua9c0\uaa00-\uaa27\uaa40-\uaa41\uaa4c-\uaa4d\uaa50-\uaa59\uaa7b\uaae0-\uaae9\uaaf2-\uaaf3\uabc0-\uabe1\uabec\uabed\uabf0-\uabf9\ufb20-\ufb28\ufe00-\ufe0f\ufe20-\ufe26\ufe33\ufe34\ufe4d-\ufe4f\uff10-\uff19\uff3f])*";
const IDENTIFIER_MATCH: &str = r"(?:\\u[0-9a-fA-F]{4}|\\u\{[0-9a-fA-F]+\}|[\x24\x30-\x39\x41-\x5a\x5f\x61-\x7a\xaa\xb5\xba\xc0-\xd6\xd8-\xf6\xf8-\u02c1\u02c6-\u02d1\u02e0-\u02e4\u02ec\u02ee\u0370-\u0374\u0376\u0377\u037a-\u037d\u0386\u0388-\u038a\u038c\u038e-\u03a1\u03a3-\u03f5\u03f7-\u0481\u048a-\u0527\u0531-\u0556\u0559\u0561-\u0587\u05d0-\u05ea\u05f0-\u05f2\u0620-\u064a\u066e\u066f\u0671-\u06d3\u06d5\u06e5\u06e6\u06ee\u06ef\u06fa-\u06fc\u06ff\u0710\u0712-\u072f\u074d-\u07a5\u07b1\u07ca-\u07ea\u07f4\u07f5\u07fa\u0800-\u0815\u081a\u0824\u0828\u0840-\u0858\u08a0\u08a2-\u08ac\u0904-\u0939\u093d\u0950\u0958-\u0961\u0971-\u0977\u0979-\u097f\u0985-\u098c\u098f\u0990\u0993-\u09a8\u09aa-\u09b0\u09b2\u09b6-\u09b9\u09bd\u09ce\u09dc\u09dd\u09df-\u09e1\u09f0\u09f1\u0a05-\u0a0a\u0a0f\u0a10\u0a13-\u0a28\u0a2a-\u0a30\u0a32\u0a33\u0a35\u0a36\u0a38\u0a39\u0a59-\u0a5c\u0a5e\u0a72-\u0a74\u0a85-\u0a8d\u0a8f-\u0a91\u0a93-\u0aa8\u0aaa-\u0ab0\u0ab2\u0ab3\u0ab5-\u0ab9\u0abd\u0ad0\u0ae0\u0ae1\u0b05-\u0b0c\u0b0f\u0b10\u0b13-\u0b28\u0b2a-\u0b30\u0b32\u0b33\u0b35-\u0b39\u0b3d\u0b5c\u0b5d\u0b5f-\u0b61\u0b71\u0b83\u0b85-\u0b8a\u0b8e-\u0b90\u0b92-\u0b95\u0b99\u0b9a\u0b9c\u0b9e\u0b9f\u0ba3\u0ba4\u0ba8-\u0baa\u0bae-\u0bb9\u0bd0\u0c05-\u0c0c\u0c0e-\u0c10\u0c12-\u0c28\u0c2a-\u0c33\u0c35-\u0c39\u0c3d\u0c58\u0c59\u0c60\u0c61\u0c85-\u0c8c\u0c8e-\u0c90\u0c92-\u0ca8\u0caa-\u0cb3\u0cb5-\u0cb9\u0cbd\u0cde\u0ce0\u0ce1\u0cf1\u0cf2\u0d05-\u0d0c\u0d0e-\u0d10\u0d12-\u0d3a\u0d3d\u0d4e\u0d60\u0d61\u0d7a-\u0d7f\u0d85-\u0d96\u0d9a-\u0db1\u0db3-\u0dbb\u0dbd\u0dc0-\u0dc6\u0e01-\u0e30\u0e32\u0e33\u0e40-\u0e46\u0e81\u0e82\u0e84\u0e87\u0e88\u0e8a\u0e8d\u0e94-\u0e97\u0e99-\u0e9f\u0ea1-\u0ea3\u0ea5\u0ea7\u0eaa\u0eab\u0ead-\u0eb0\u0eb2\u0eb3\u0ebd\u0ec0-\u0ec4\u0ec6\u0edc-\u0edf\u0f00\u0f40-\u0f47\u0f49-\u0f6c\u0f88-\u0f8c\u1000-\u102a\u103f\u1050-\u1055\u105a-\u105d\u1061\u1065\u1066\u106e-\u1070\u1075-\u1081\u108e\u10a0-\u10c5\u10c7\u10cd\u10d0-\u10fa\u10fc-\u1248\u124a-\u124d\u1250-\u1256\u1258\u125a-\u125d\u1260-\u1288\u128a-\u128d\u1290-\u12b0\u12b2-\u12b5\u12b8-\u12be\u12c0\u12c2-\u12c5\u12c8-\u12d6\u12d8-\u1310\u1312-\u1315\u1318-\u135a\u1380-\u138f\u13a0-\u13f4\u1401-\u166c\u166f-\u167f\u1681-\u169a\u16a0-\u16ea\u16ee-\u16f0\u1700-\u170c\u170e-\u1711\u1720-\u1731\u1740-\u1751\u1760-\u176c\u176e-\u1770\u1780-\u17b3\u17d7\u17dc\u1820-\u1877\u1880-\u18a8\u18aa\u18b0-\u18f5\u1900-\u191c\u1950-\u196d\u1970-\u1974\u1980-\u19ab\u19c1-\u19c7\u1a00-\u1a16\u1a20-\u1a54\u1aa7\u1b05-\u1b33\u1b45-\u1b4b\u1b83-\u1ba0\u1bae\u1baf\u1bba-\u1be5\u1c00-\u1c23\u1c4d-\u1c4f\u1c5a-\u1c7d\u1ce9-\u1cec\u1cee-\u1cf1\u1cf5\u1cf6\u1d00-\u1dbf\u1e00-\u1f15\u1f18-\u1f1d\u1f20-\u1f45\u1f48-\u1f4d\u1f50-\u1f57\u1f59\u1f5b\u1f5d\u1f5f-\u1f7d\u1f80-\u1fb4\u1fb6-\u1fbc\u1fbe\u1fc2-\u1fc4\u1fc6-\u1fcc\u1fd0-\u1fd3\u1fd6-\u1fdb\u1fe0-\u1fec\u1ff2-\u1ff4\u1ff6-\u1ffc\u2071\u207f\u2090-\u209c\u2102\u2107\u210a-\u2113\u2115\u2119-\u211d\u2124\u2126\u2128\u212a-\u212d\u212f-\u2139\u213c-\u213f\u2145-\u2149\u214e\u2160-\u2188\u2c00-\u2c2e\u2c30-\u2c5e\u2c60-\u2ce4\u2ceb-\u2cee\u2cf2\u2cf3\u2d00-\u2d25\u2d27\u2d2d\u2d30-\u2d67\u2d6f\u2d80-\u2d96\u2da0-\u2da6\u2da8-\u2dae\u2db0-\u2db6\u2db8-\u2dbe\u2dc0-\u2dc6\u2dc8-\u2dce\u2dd0-\u2dd6\u2dd8-\u2dde\u2e2f\u3005-\u3007\u3021-\u3029\u3031-\u3035\u3038-\u303c\u3041-\u3096\u309d-\u309f\u30a1-\u30fa\u30fc-\u30ff\u3105-\u312d\u3131-\u318e\u31a0-\u31ba\u31f0-\u31ff\u3400-\u4db5\u4e00-\u9fcc\ua000-\ua48c\ua4d0-\ua4fd\ua500-\ua60c\ua610-\ua61f\ua62a\ua62b\ua640-\ua66e\ua67f-\ua697\ua6a0-\ua6ef\ua717-\ua71f\ua722-\ua788\ua78b-\ua78e\ua790-\ua793\ua7a0-\ua7aa\ua7f8-\ua801\ua803-\ua805\ua807-\ua80a\ua80c-\ua822\ua840-\ua873\ua882-\ua8b3\ua8f2-\ua8f7\ua8fb\ua90a-\ua925\ua930-\ua946\ua960-\ua97c\ua984-\ua9b2\ua9cf\uaa00-\uaa28\uaa40-\uaa42\uaa44-\uaa4b\uaa60-\uaa76\uaa7a\uaa80-\uaaaf\uaab1\uaab5\uaab6\uaab9-\uaabd\uaac0\uaac2\uaadb-\uaadd\uaae0-\uaaea\uaaf2-\uaaf4\uab01-\uab06\uab09-\uab0e\uab11-\uab16\uab20-\uab26\uab28-\uab2e\uabc0-\uabe2\uac00-\ud7a3\ud7b0-\ud7c6\ud7cb-\ud7fb\uf900-\ufa6d\ufa70-\ufad9\ufb00-\ufb06\ufb13-\ufb17\ufb1d\ufb1f-\ufb28\ufb2a-\ufb36\ufb38-\ufb3c\ufb3e\ufb40\ufb41\ufb43\ufb44\ufb46-\ufbb1\ufbd3-\ufd3d\ufd50-\ufd8f\ufd92-\ufdc7\ufdf0-\ufdfb\ufe70-\ufe74\ufe76-\ufefc\uff21-\uff3a\uff41-\uff5a\uff66-\uffbe\uffc2-\uffc7\uffca-\uffcf\uffd2-\uffd7\uffda-\uffdc\u0300-\u036f\u0483-\u0487\u0591-\u05bd\u05bf\u05c1\u05c2\u05c4\u05c5\u05c7\u0610-\u061a\u0620-\u0649\u0672-\u06d3\u06e7-\u06e8\u06fb-\u06fc\u0730-\u074a\u0800-\u0814\u081b-\u0823\u0825-\u0827\u0829-\u082d\u0840-\u0857\u08e4-\u08fe\u0900-\u0903\u093a-\u093c\u093e-\u094f\u0951-\u0957\u0962-\u0963\u0966-\u096f\u0981-\u0983\u09bc\u09be-\u09c4\u09c7\u09c8\u09d7\u09df-\u09e0\u0a01-\u0a03\u0a3c\u0a3e-\u0a42\u0a47\u0a48\u0a4b-\u0a4d\u0a51\u0a66-\u0a71\u0a75\u0a81-\u0a83\u0abc\u0abe-\u0ac5\u0ac7-\u0ac9\u0acb-\u0acd\u0ae2-\u0ae3\u0ae6-\u0aef\u0b01-\u0b03\u0b3c\u0b3e-\u0b44\u0b47\u0b48\u0b4b-\u0b4d\u0b56\u0b57\u0b5f-\u0b60\u0b66-\u0b6f\u0b82\u0bbe-\u0bc2\u0bc6-\u0bc8\u0bca-\u0bcd\u0bd7\u0be6-\u0bef\u0c01-\u0c03\u0c46-\u0c48\u0c4a-\u0c4d\u0c55\u0c56\u0c62-\u0c63\u0c66-\u0c6f\u0c82\u0c83\u0cbc\u0cbe-\u0cc4\u0cc6-\u0cc8\u0cca-\u0ccd\u0cd5\u0cd6\u0ce2-\u0ce3\u0ce6-\u0cef\u0d02\u0d03\u0d46-\u0d48\u0d57\u0d62-\u0d63\u0d66-\u0d6f\u0d82\u0d83\u0dca\u0dcf-\u0dd4\u0dd6\u0dd8-\u0ddf\u0df2\u0df3\u0e34-\u0e3a\u0e40-\u0e45\u0e50-\u0e59\u0eb4-\u0eb9\u0ec8-\u0ecd\u0ed0-\u0ed9\u0f18\u0f19\u0f20-\u0f29\u0f35\u0f37\u0f39\u0f41-\u0f47\u0f71-\u0f84\u0f86-\u0f87\u0f8d-\u0f97\u0f99-\u0fbc\u0fc6\u1000-\u1029\u1040-\u1049\u1067-\u106d\u1071-\u1074\u1082-\u108d\u108f-\u109d\u135d-\u135f\u170e-\u1710\u1720-\u1730\u1740-\u1750\u1772\u1773\u1780-\u17b2\u17dd\u17e0-\u17e9\u180b-\u180d\u1810-\u1819\u1920-\u192b\u1930-\u193b\u1951-\u196d\u19b0-\u19c0\u19c8-\u19c9\u19d0-\u19d9\u1a00-\u1a15\u1a20-\u1a53\u1a60-\u1a7c\u1a7f-\u1a89\u1a90-\u1a99\u1b46-\u1b4b\u1b50-\u1b59\u1b6b-\u1b73\u1bb0-\u1bb9\u1be6-\u1bf3\u1c00-\u1c22\u1c40-\u1c49\u1c5b-\u1c7d\u1cd0-\u1cd2\u1d00-\u1dbe\u1e01-\u1f15\u200c\u200d\u203f\u2040\u2054\u20d0-\u20dc\u20e1\u20e5-\u20f0\u2d81-\u2d96\u2de0-\u2dff\u3021-\u3028\u3099\u309a\ua640-\ua66d\ua674-\ua67d\ua69f\ua6f0-\ua6f1\ua7f8-\ua800\ua806\ua80b\ua823-\ua827\ua880-\ua881\ua8b4-\ua8c4\ua8d0-\ua8d9\ua8f3-\ua8f7\ua900-\ua909\ua926-\ua92d\ua930-\ua945\ua980-\ua983\ua9b3-\ua9c0\uaa00-\uaa27\uaa40-\uaa41\uaa4c-\uaa4d\uaa50-\uaa59\uaa7b\uaae0-\uaae9\uaaf2-\uaaf3\uabc0-\uabe1\uabec\uabed\uabf0-\uabf9\ufb20-\ufb28\ufe00-\ufe0f\ufe20-\ufe26\ufe33\ufe34\ufe4d-\ufe4f\uff10-\uff19\uff3f])+";

const NEWLINE_CHARS: &str = "\n\r\u{2028}\u{2029}";
const LINE_BREAK: &str = r"\r\n|[\n\r  ]";

fn is_newline(c: char) -> bool {
    NEWLINE_CHARS.contains(c)
}

/// `s.replace(/\r\n|[\n\r  ]/g, '\n')`
fn normalize_line_breaks(s: &str) -> String {
    if !s.contains(['\r', '\u{2028}', '\u{2029}']) {
        return s.to_string();
    }
    s.replace("\r\n", "\n").replace(['\r', '\u{2028}', '\u{2029}'], "\n")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum K {
    StartExpr,
    EndExpr,
    StartBlock,
    EndBlock,
    Word,
    Reserved,
    Semicolon,
    Str,
    Equals,
    Operator,
    Comma,
    BlockComment,
    Comment,
    Dot,
    Unknown,
    Start,
    Eof,
}

#[derive(Clone, Debug)]
struct Token {
    kind: K,
    text: String,
    newlines: usize,
    whitespace_before: String,
    comments_before: Vec<usize>,
    comments_pos: usize,
    previous: Option<usize>,
    opened: Option<usize>,
    directives: Option<Vec<(String, String)>>,
}

impl Token {
    fn new(kind: K, text: &str) -> Token {
        Token {
            kind,
            text: text.to_string(),
            newlines: 0,
            whitespace_before: String::new(),
            comments_before: Vec::new(),
            comments_pos: 0,
            previous: None,
            opened: None,
            directives: None,
        }
    }

    fn directive(&self, name: &str) -> Option<&str> {
        self.directives.as_ref()?.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

const LINE_STARTERS: &[&str] = &[
    "continue", "try", "throw", "return", "var", "let", "const", "if", "switch", "case", "default", "for",
    "while", "break", "function", "import", "export",
];
const RESERVED_EXTRA: &[&str] = &[
    "do", "in", "of", "else", "get", "set", "new", "catch", "finally", "typeof", "yield", "async", "await",
    "from", "as", "class", "extends",
];
const POSITIONABLE_OPERATORS: &[&str] = &[
    ">>>", "===", "!==", "&&=", "??=", "||=", "<<", "&&", ">=", "**", "!=", "==", "<=", ">>", "||", "??",
    "|>", "<", "/", "-", "+", ">", ":", "&", "%", "?", "^", "|", "*",
];
const PUNCT: &[&str] = &[
    ">>>=", "...", ">>=", "<<=", "===", ">>>", "!==", "**=", "&&=", "??=", "||=", "=>", "^=", "::", "/=",
    "<<", "<=", "==", "&&", "-=", ">=", ">>", "!=", "--", "+=", "**", "||", "??", "++", "%=", "&=", "*=",
    "|=", "|>", "=", "!", "?", ">", "<", ":", "/", "^", "-", "+", "*", "&", "%", "~", "|",
];
const NUMBER: &str = r"0[xX][0123456789abcdefABCDEF_]*n?|0[oO][01234567_]*n?|0[bB][01_]*n?|\d[\d_]*n|(?:\.\d[\d_]*|\d[\d_]*\.?[\d_]*)(?:[eE][+-]?[\d_]+)?";

fn is_reserved(word: &str) -> bool {
    LINE_STARTERS.contains(&word) || RESERVED_EXTRA.contains(&word)
}

// ------------------------------------------------------------------ tokenizer

struct Patterns {
    identifier: TemplatablePattern,
    number: Pattern,
    punct: Re,
    comment: Pattern,
    block_comment: Pattern,
    html_comment_start: Pattern,
    html_comment_end: Pattern,
    include: Pattern,
    shebang: Pattern,
    single_quote: TemplatablePattern,
    double_quote: TemplatablePattern,
    template_text: TemplatablePattern,
    template_expression: TemplatablePattern,
    identifier_re: Re,
}

struct Tokenizer {
    input: InputScanner,
    whitespace: WhitespacePattern,
    patterns: Patterns,
    directives: Directives,
    in_html_comment: bool,
    has_char_escapes: bool,
    tokens: Vec<Token>,
    /// The main token stream (indexes into `tokens`; comments are attached
    /// to the following token instead).
    stream: Vec<usize>,
}

impl Tokenizer {
    fn new(text: &str, templating: &[String]) -> Tokenizer {
        let templatable = TemplatablePattern::new().read_options(templating);
        let p = Pattern::default();
        let punct: Vec<String> = PUNCT.iter().map(|s| super::core::escape(s)).collect();
        Tokenizer {
            input: InputScanner::new(text),
            whitespace: WhitespacePattern::matching(
                "\u{00A0}\u{1680}\u{180e}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}\u{202f}\u{205f}\u{3000}\u{feff}",
                "\u{2028}\u{2029}",
            ),
            patterns: Patterns {
                identifier: templatable.starting_with(IDENTIFIER).matching(IDENTIFIER_MATCH),
                number: p.matching(NUMBER),
                punct: Re::new(&punct.join("|")),
                comment: p.starting_with("//").until(r"[\n\r  ]"),
                block_comment: p.starting_with(r"/\*").until_after(r"\*/"),
                html_comment_start: p.matching("<!--"),
                html_comment_end: p.matching("-->"),
                include: p.starting_with("#include").until_after(LINE_BREAK),
                shebang: p.starting_with("#!").until_after(LINE_BREAK),
                single_quote: templatable.until(r"['\\\n\r  ]"),
                double_quote: templatable.until(r#"["\\\n\r  ]"#),
                template_text: templatable.until(r"[`\\$]"),
                template_expression: templatable.until(r"[`}\\]"),
                identifier_re: Re::new(IDENTIFIER),
            },
            directives: Directives::new(r"/\*", r"\*/"),
            in_html_comment: false,
            has_char_escapes: false,
            tokens: Vec::new(),
            stream: Vec::new(),
        }
    }

    fn create(&self, kind: K, text: &str) -> Token {
        let mut t = Token::new(kind, text);
        t.newlines = self.whitespace.newline_count;
        t.whitespace_before = self.whitespace.whitespace_before_token.clone();
        t
    }

    fn push(&mut self, t: Token) -> usize {
        self.tokens.push(t);
        self.tokens.len() - 1
    }

    fn tokenize(mut self) -> (Vec<Token>, Vec<usize>) {
        self.input.restart();
        self.in_html_comment = false;
        let start = Token::new(K::Start, "");
        let mut previous = self.push(start);
        let mut open: Option<usize> = None;
        let mut open_stack: Vec<Option<usize>> = Vec::new();
        let mut comments: Vec<usize> = Vec::new();
        while self.tokens[previous].kind != K::Eof {
            let mut current = self.next_token(previous);
            while matches!(current.kind, K::Comment | K::BlockComment | K::Unknown) {
                let c = self.push(current);
                comments.push(c);
                current = self.next_token(previous);
            }
            current.comments_before = std::mem::take(&mut comments);
            let is_opening = matches!(current.kind, K::StartBlock | K::StartExpr);
            let index = self.tokens.len();
            if is_opening {
                open_stack.push(open);
                open = Some(index);
            } else if let Some(o) = open {
                let ot = self.tokens[o].text.as_str();
                if matches!(current.kind, K::EndBlock | K::EndExpr)
                    && ((current.text == "]" && ot == "[")
                        || (current.text == ")" && ot == "(")
                        || (current.text == "}" && ot == "{"))
                {
                    current.opened = Some(o);
                    open = open_stack.pop().flatten();
                }
            }
            current.previous = Some(previous);
            let i = self.push(current);
            self.stream.push(i);
            previous = i;
        }
        (self.tokens, self.stream)
    }

    fn next_token(&mut self, previous: usize) -> Token {
        self.whitespace.read(&mut self.input);
        let Some(c) = self.input.peek0() else { return self.create(K::Eof, "") };
        if let Some(t) = self.read_non_javascript(c) {
            return t;
        }
        if let Some(t) = self.read_string(c) {
            return t;
        }
        if let Some(t) = self.read_pair(c, self.input.peek(1)) {
            return t;
        }
        if let Some(t) = self.read_word(previous) {
            return t;
        }
        if let Some(t) = self.read_singles(c) {
            return t;
        }
        if let Some(t) = self.read_comment(c) {
            return t;
        }
        if let Some(t) = self.read_regexp(c, previous) {
            return t;
        }
        if let Some(t) = self.read_punctuation() {
            return t;
        }
        let c = self.input.next().map(String::from).unwrap_or_default();
        self.create(K::Unknown, &c)
    }

    fn read_word(&mut self, previous: usize) -> Option<Token> {
        let s = self.patterns.identifier.read(&mut self.input);
        if !s.is_empty() {
            let s = normalize_line_breaks(&s);
            let prev = &self.tokens[previous];
            if !(prev.kind == K::Dot
                || (prev.kind == K::Reserved && (prev.text == "set" || prev.text == "get")))
                && is_reserved(&s)
            {
                if (s == "in" || s == "of") && matches!(prev.kind, K::Word | K::Str) {
                    return Some(self.create(K::Operator, &s));
                }
                return Some(self.create(K::Reserved, &s));
            }
            return Some(self.create(K::Word, &s));
        }
        let s = self.patterns.number.read(&mut self.input);
        if !s.is_empty() {
            return Some(self.create(K::Word, &s));
        }
        None
    }

    fn read_singles(&mut self, c: char) -> Option<Token> {
        let kind = match c {
            '(' | '[' => K::StartExpr,
            ')' | ']' => K::EndExpr,
            '{' => K::StartBlock,
            '}' => K::EndBlock,
            ';' => K::Semicolon,
            // `.` but not `...` or a decimal
            '.' if self.input.peek(1).is_none_or(|n| !n.is_ascii_digit() && n != '.') => K::Dot,
            ',' => K::Comma,
            _ => return None,
        };
        let t = self.create(kind, &c.to_string());
        self.input.next();
        Some(t)
    }

    fn read_pair(&mut self, c: char, d: Option<char>) -> Option<Token> {
        if c == '#' && d == Some('{') {
            let t = self.create(K::StartBlock, "#{");
            self.input.next();
            self.input.next();
            return Some(t);
        }
        None
    }

    fn read_punctuation(&mut self) -> Option<Token> {
        let pos = self.input.position();
        let text = self.input.text();
        // `?.` but not when followed by a digit
        let s = if text[pos..].starts_with("?.") && !text[pos + 2..].starts_with(|c: char| c.is_ascii_digit())
        {
            Some("?.".to_string())
        } else {
            self.patterns.punct.match_at(text, pos).map(str::to_string)
        }?;
        if s.is_empty() {
            return None;
        }
        self.input.set_position(pos + s.len());
        let kind = match s.as_str() {
            "=" => K::Equals,
            "?." => K::Dot,
            _ => K::Operator,
        };
        Some(self.create(kind, &s))
    }

    fn read_non_javascript(&mut self, c: char) -> Option<Token> {
        if c == '#' {
            if self.stream.is_empty() {
                let s = self.patterns.shebang.read(&mut self.input);
                if !s.is_empty() {
                    return Some(self.create(K::Unknown, &format!("{}\n", s.trim())));
                }
            }
            let s = self.patterns.include.read(&mut self.input);
            if !s.is_empty() {
                return Some(self.create(K::Unknown, &format!("{}\n", s.trim())));
            }
            self.input.next();
            let mut sharp = String::from("#");
            if self.input.has_next() && self.input.peek0().is_some_and(|c| c.is_ascii_digit()) {
                let mut c;
                loop {
                    c = self.input.next();
                    if let Some(ch) = c {
                        sharp.push(ch);
                    }
                    if !(self.input.has_next() && c != Some('#') && c != Some('=')) {
                        break;
                    }
                }
                if c == Some('#') {
                } else if self.input.peek0() == Some('[') && self.input.peek(1) == Some(']') {
                    sharp.push_str("[]");
                    self.input.next();
                    self.input.next();
                } else if self.input.peek0() == Some('{') && self.input.peek(1) == Some('}') {
                    sharp.push_str("{}");
                    self.input.next();
                    self.input.next();
                }
                return Some(self.create(K::Word, &sharp));
            }
            self.input.back();
        } else if c == '<' && self.stream.is_empty() {
            let mut s = self.patterns.html_comment_start.read(&mut self.input);
            if !s.is_empty() {
                while self.input.has_next() && !self.input.peek0().is_some_and(is_newline) {
                    if let Some(n) = self.input.next() {
                        s.push(n);
                    }
                }
                self.in_html_comment = true;
                return Some(self.create(K::Comment, &s));
            }
        } else if self.in_html_comment && c == '-' {
            let s = self.patterns.html_comment_end.read(&mut self.input);
            if !s.is_empty() {
                self.in_html_comment = false;
                return Some(self.create(K::Comment, &s));
            }
        }
        None
    }

    fn read_comment(&mut self, c: char) -> Option<Token> {
        if c != '/' {
            return None;
        }
        match self.input.peek(1) {
            Some('*') => {
                let mut comment = self.patterns.block_comment.read(&mut self.input);
                let directives = self.directives.get(&comment);
                if directives.as_ref().is_some_and(|d| {
                    d.iter().rev().find(|(k, _)| k == "ignore").is_some_and(|(_, v)| v == "start")
                }) {
                    comment.push_str(&self.directives.read_ignored(&mut self.input));
                }
                let comment = normalize_line_breaks(&comment);
                let mut t = self.create(K::BlockComment, &comment);
                t.directives = directives;
                Some(t)
            }
            Some('/') => {
                let comment = self.patterns.comment.read(&mut self.input);
                Some(self.create(K::Comment, &comment))
            }
            _ => None,
        }
    }

    fn read_string(&mut self, c: char) -> Option<Token> {
        if c != '`' && c != '\'' && c != '"' {
            return None;
        }
        let mut s = String::from(self.input.next()?);
        self.has_char_escapes = false;
        if c == '`' {
            s.push_str(&self.read_string_recursive('`', true, Some("${")));
        } else {
            s.push_str(&self.read_string_recursive(c, false, None));
        }
        if self.input.peek0() == Some(c) {
            if let Some(n) = self.input.next() {
                s.push(n);
            }
        }
        let s = normalize_line_breaks(&s);
        Some(self.create(K::Str, &s))
    }

    fn allow_regexp_or_xml(&self, previous: usize) -> bool {
        let p = &self.tokens[previous];
        (p.kind == K::Reserved
            && ["return", "case", "throw", "else", "do", "typeof", "yield"].contains(&p.text.as_str()))
            || (p.kind == K::EndExpr
                && p.text == ")"
                && p.opened.and_then(|o| self.tokens[o].previous).is_some_and(|pp| {
                    let pp = &self.tokens[pp];
                    pp.kind == K::Reserved && ["if", "while", "for"].contains(&pp.text.as_str())
                }))
            || matches!(
                p.kind,
                K::Comment
                    | K::StartExpr
                    | K::StartBlock
                    | K::Start
                    | K::EndBlock
                    | K::Operator
                    | K::Equals
                    | K::Eof
                    | K::Semicolon
                    | K::Comma
            )
    }

    fn read_regexp(&mut self, c: char, previous: usize) -> Option<Token> {
        if c != '/' || !self.allow_regexp_or_xml(previous) {
            return None;
        }
        let mut s = String::from(self.input.next()?);
        let mut esc = false;
        let mut in_char_class = false;
        while self.input.has_next()
            && (esc || in_char_class || self.input.peek0() != Some(c))
            && !self.input.peek0().is_some_and(is_newline)
        {
            let p = self.input.peek0()?;
            s.push(p);
            if !esc {
                esc = p == '\\';
                if p == '[' {
                    in_char_class = true;
                } else if p == ']' {
                    in_char_class = false;
                }
            } else {
                esc = false;
            }
            self.input.next();
        }
        if self.input.peek0() == Some(c) {
            if let Some(n) = self.input.next() {
                s.push(n);
            }
            let id = self.patterns.identifier_re.clone();
            s.push_str(&self.input.read(Some(&id), None, false));
        }
        Some(self.create(K::Str, &s))
    }

    fn read_string_recursive(
        &mut self,
        delimiter: char,
        allow_unescaped_newlines: bool,
        start_sub: Option<&str>,
    ) -> String {
        let pattern = match delimiter {
            '\'' => self.patterns.single_quote.clone(),
            '"' => self.patterns.double_quote.clone(),
            '`' => self.patterns.template_text.clone(),
            _ => self.patterns.template_expression.clone(),
        };
        let mut s = pattern.read(&mut self.input);
        while self.input.has_next() {
            let Some(c) = self.input.next() else { break };
            let mut next = String::from(c);
            if c == delimiter || (!allow_unescaped_newlines && is_newline(c)) {
                self.input.back();
                break;
            } else if c == '\\' && self.input.has_next() {
                let current = self.input.peek0();
                if current == Some('x') || current == Some('u') {
                    self.has_char_escapes = true;
                } else if current == Some('\r') && self.input.peek(1) == Some('\n') {
                    self.input.next();
                }
                if let Some(n) = self.input.next() {
                    next.push(n);
                }
            } else if let Some(sub) = start_sub {
                if sub == "${" && c == '$' && self.input.peek0() == Some('{') {
                    if let Some(n) = self.input.next() {
                        next.push(n);
                    }
                }
                if sub == next {
                    if delimiter == '`' {
                        next.push_str(&self.read_string_recursive('}', allow_unescaped_newlines, Some("`")));
                    } else {
                        next.push_str(&self.read_string_recursive('`', allow_unescaped_newlines, Some("${")));
                    }
                    if self.input.has_next() {
                        if let Some(n) = self.input.next() {
                            next.push(n);
                        }
                    }
                }
            }
            next.push_str(&pattern.read(&mut self.input));
            s.push_str(&next);
        }
        s
    }
}

// ------------------------------------------------------------------ beautifier

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    BlockStatement,
    Statement,
    ObjectLiteral,
    ArrayLiteral,
    ForInitializer,
    Conditional,
    Expression,
}

fn is_array(mode: Mode) -> bool {
    mode == Mode::ArrayLiteral
}

fn is_expression(mode: Mode) -> bool {
    matches!(mode, Mode::Expression | Mode::ForInitializer | Mode::Conditional)
}

#[derive(Clone, Debug)]
struct Flags {
    mode: Mode,
    parent: Option<usize>,
    last_token: usize,
    last_word: String,
    declaration_statement: bool,
    declaration_assignment: bool,
    multiline_frame: bool,
    inline_frame: bool,
    if_block: bool,
    else_block: bool,
    class_start_block: bool,
    do_block: bool,
    do_while: bool,
    import_block: bool,
    in_case_statement: bool,
    in_case: bool,
    case_body: bool,
    case_block: bool,
    indentation_level: isize,
    alignment: usize,
    line_indent_level: isize,
    start_line_index: usize,
    ternary_depth: usize,
}

const SPECIAL_WORDS: &[&str] =
    &["case", "return", "do", "if", "throw", "else", "await", "break", "continue", "async"];
const NEWLINE_RESTRICTED: &[&str] = &["async", "break", "continue", "return", "throw", "yield"];

/// The options a `<script>` gets: js-beautify's defaults except for the
/// indentation (`brace_style: collapse`, `operator_position:
/// before-newline`, no wrapping, `preserve_newlines`).
struct Options {
    base: BaseOptions,
}

#[derive(PartialEq, Eq)]
enum Prefix {
    None,
    Space,
    Newline,
}

struct Beautifier {
    options: Options,
    output: Output,
    tokens: Vec<Token>,
    stream: Vec<usize>,
    position: usize,
    last_last_text: String,
    flags_arena: Vec<Flags>,
    flags: usize,
    previous_flags: usize,
    flag_store: Vec<usize>,
}

fn php_adjustment(text: &str) -> Option<isize> {
    let rest = text.strip_prefix("<?php ")?.strip_suffix(" ?>")?;
    let digits = rest.strip_prefix('-').unwrap_or(rest);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

impl Beautifier {
    fn f(&self) -> &Flags {
        &self.flags_arena[self.flags]
    }

    fn fm(&mut self) -> &mut Flags {
        &mut self.flags_arena[self.flags]
    }

    fn pf(&self) -> &Flags {
        &self.flags_arena[self.previous_flags]
    }

    fn parent_mode(&self) -> Option<Mode> {
        self.f().parent.map(|p| self.flags_arena[p].mode)
    }

    fn last(&self) -> &Token {
        &self.tokens[self.f().last_token]
    }

    fn last_is(&self, kind: K) -> bool {
        self.last().kind == kind
    }

    fn last_text_is(&self, text: &str) -> bool {
        self.last().text == text
    }

    fn reserved(&self, t: Option<usize>, word: &str) -> bool {
        t.is_some_and(|t| self.tokens[t].kind == K::Reserved && self.tokens[t].text == word)
    }

    fn reserved_any(&self, t: Option<usize>, words: &[&str]) -> bool {
        t.is_some_and(|t| self.tokens[t].kind == K::Reserved && words.contains(&self.tokens[t].text.as_str()))
    }

    fn last_reserved(&self, word: &str) -> bool {
        self.reserved(Some(self.f().last_token), word)
    }

    fn last_reserved_any(&self, words: &[&str]) -> bool {
        self.reserved_any(Some(self.f().last_token), words)
    }

    /// `this._tokens.peek(index)`: relative to the next token.
    fn peek(&self, index: isize) -> Option<usize> {
        let i = self.position as isize + index;
        if i < 0 {
            return None;
        }
        self.stream.get(i as usize).copied()
    }

    fn create_flags(&mut self, base: Option<usize>, mode: Mode) -> usize {
        let mut next_indent_level = 0;
        if let Some(b) = base {
            let b = &self.flags_arena[b];
            next_indent_level = b.indentation_level;
            if !self.output.just_added_newline() && b.line_indent_level > next_indent_level {
                next_indent_level = b.line_indent_level;
            }
        }
        let (last_token, last_word, line_indent_level) = match base {
            Some(b) => {
                let b = &self.flags_arena[b];
                (b.last_token, b.last_word.clone(), b.line_indent_level)
            }
            None => {
                self.tokens.push(Token::new(K::StartBlock, ""));
                (self.tokens.len() - 1, String::new(), next_indent_level)
            }
        };
        self.flags_arena.push(Flags {
            mode,
            parent: base,
            last_token,
            last_word,
            declaration_statement: false,
            declaration_assignment: false,
            multiline_frame: false,
            inline_frame: false,
            if_block: false,
            else_block: false,
            class_start_block: false,
            do_block: false,
            do_while: false,
            import_block: false,
            in_case_statement: false,
            in_case: false,
            case_body: false,
            case_block: false,
            indentation_level: next_indent_level,
            alignment: 0,
            line_indent_level,
            start_line_index: self.output.get_line_number(),
            ternary_depth: 0,
        });
        self.flags_arena.len() - 1
    }

    fn beautify(text: &str, options: Options) -> String {
        let base_indent: String = text.chars().take_while(|c| *c == '\t' || *c == ' ').collect();
        let output = Output::new(&options.base, &base_indent);
        let (tokens, stream) = Tokenizer::new(text, &options.base.templating).tokenize();
        let mut b = Beautifier {
            options,
            output,
            tokens,
            stream,
            position: 0,
            last_last_text: String::new(),
            flags_arena: Vec::new(),
            flags: usize::MAX,
            previous_flags: usize::MAX,
            flag_store: Vec::new(),
        };
        b.set_mode(Mode::BlockStatement);
        while b.position < b.stream.len() {
            let current = b.stream[b.position];
            b.position += 1;
            b.handle_token(current, false);
            b.last_last_text = b.last().text.clone();
            b.fm().last_token = current;
        }
        b.output.get_code("\n")
    }

    fn handle_token(&mut self, t: usize, preserve: bool) {
        match self.tokens[t].kind {
            K::StartExpr => self.handle_start_expr(t),
            K::EndExpr => self.handle_end_expr(t),
            K::StartBlock => self.handle_start_block(t),
            K::EndBlock => self.handle_end_block(t),
            K::Word | K::Reserved => self.handle_word(t),
            K::Semicolon => self.handle_semicolon(t),
            K::Str => self.handle_string(t),
            K::Equals => self.handle_equals(t),
            K::Operator => self.handle_operator(t),
            K::Comma => self.handle_comma(t),
            K::BlockComment => self.handle_block_comment(t, preserve),
            K::Comment => self.handle_comment(t, preserve),
            K::Dot => self.handle_dot(t),
            K::Eof => self.handle_eof(t),
            _ => self.handle_unknown(t, preserve),
        }
    }

    fn handle_whitespace_and_comments(&mut self, t: usize, preserve: bool) {
        let mut newlines = self.tokens[t].newlines;
        while self.tokens[t].comments_pos < self.tokens[t].comments_before.len() {
            let c = self.tokens[t].comments_before[self.tokens[t].comments_pos];
            self.tokens[t].comments_pos += 1;
            self.handle_whitespace_and_comments(c, preserve);
            self.handle_token(c, preserve);
        }
        let max = self.options.base.max_preserve_newlines;
        if max > 0 && newlines > max {
            newlines = max;
        }
        if self.options.base.preserve_newlines && newlines > 1 {
            self.print_newline(false, preserve);
            for _ in 1..newlines {
                self.print_newline(true, preserve);
            }
        }
    }

    fn allow_wrap_or_preserved_newline(&mut self, t: usize, force_linewrap: bool) {
        if self.output.just_added_newline() {
            return;
        }
        let current = &self.tokens[t];
        let mut should = (self.options.base.preserve_newlines && current.newlines > 0) || force_linewrap;
        let last_positionable = POSITIONABLE_OPERATORS.contains(&self.last().text.as_str());
        let current_positionable = POSITIONABLE_OPERATORS.contains(&current.text.as_str());
        if last_positionable || current_positionable {
            // operator_position: before-newline
            let should_print_operator_newline = last_positionable || current_positionable;
            should = should && should_print_operator_newline;
        }
        if should {
            self.print_newline(false, true);
        } else if self.options.base.wrap_line_length > 0 {
            if self.last_reserved_any(NEWLINE_RESTRICTED) {
                return;
            }
            self.output.set_wrap_point();
        }
    }

    fn print_newline(&mut self, force: bool, preserve: bool) {
        if !preserve {
            let last = self.last();
            if last.text != ";"
                && last.text != ","
                && last.text != "="
                && (last.kind != K::Operator || last.text == "--" || last.text == "++")
            {
                let next = self.peek(0);
                while self.f().mode == Mode::Statement
                    && !(self.f().if_block && self.reserved(next, "else"))
                    && !self.f().do_block
                {
                    self.restore_mode();
                }
            }
        }
        if self.output.add_new_line(force) {
            self.fm().multiline_frame = true;
        }
    }

    fn print_token_line_indentation(&mut self) {
        if self.output.just_added_newline() {
            let (level, alignment) = (self.f().indentation_level, self.f().alignment);
            if self.output.set_indent(level, alignment) {
                self.fm().line_indent_level = level;
            }
        }
    }

    fn print_token(&mut self, t: usize) {
        if self.output.raw {
            let tok = &self.tokens[t];
            self.output.add_raw_token(tok.newlines, &tok.whitespace_before, &tok.text);
            return;
        }
        self.print_token_line_indentation();
        self.output.non_breaking_space = true;
        let text = self.tokens[t].text.clone();
        self.output.add_token(&text);
        if self.output.previous_token_wrapped {
            self.fm().multiline_frame = true;
        }
    }

    fn indent(&mut self) {
        self.fm().indentation_level += 1;
        let (level, alignment) = (self.f().indentation_level, self.f().alignment);
        self.output.set_indent(level, alignment);
    }

    fn deindent(&mut self) {
        let level = self.f().indentation_level;
        let parent_level = self.f().parent.map(|p| self.flags_arena[p].indentation_level);
        if level > 0 && parent_level.is_none_or(|p| level > p) {
            self.fm().indentation_level -= 1;
            let (level, alignment) = (self.f().indentation_level, self.f().alignment);
            self.output.set_indent(level, alignment);
        }
    }

    fn set_mode(&mut self, mode: Mode) {
        if self.flags != usize::MAX {
            self.flag_store.push(self.flags);
            self.previous_flags = self.flags;
        } else {
            self.previous_flags = self.create_flags(None, mode);
        }
        self.flags = self.create_flags(Some(self.previous_flags), mode);
        let (level, alignment) = (self.f().indentation_level, self.f().alignment);
        self.output.set_indent(level, alignment);
    }

    fn restore_mode(&mut self) {
        if let Some(f) = self.flag_store.pop() {
            self.previous_flags = self.flags;
            self.flags = f;
            if self.pf().mode == Mode::Statement {
                self.remove_redundant_indentation(self.previous_flags);
            }
            let (level, alignment) = (self.f().indentation_level, self.f().alignment);
            self.output.set_indent(level, alignment);
        }
    }

    fn remove_redundant_indentation(&mut self, frame: usize) {
        let f = &self.flags_arena[frame];
        if f.multiline_frame || f.mode == Mode::ForInitializer || f.mode == Mode::Conditional {
            return;
        }
        let index = f.start_line_index;
        self.output.remove_indent(index);
    }

    fn start_of_object_property(&self) -> bool {
        self.parent_mode() == Some(Mode::ObjectLiteral)
            && self.f().mode == Mode::Statement
            && ((self.last_text_is(":") && self.f().ternary_depth == 0)
                || self.last_reserved_any(&["get", "set"]))
    }

    fn start_of_statement(&mut self, t: usize) -> bool {
        let current = &self.tokens[t];
        let mut start = self.last_reserved_any(&["var", "let", "const"]) && current.kind == K::Word;
        start = start || self.last_reserved("do");
        start = start
            || (!(self.parent_mode() == Some(Mode::ObjectLiteral) && self.f().mode == Mode::Statement)
                && self.last_reserved_any(NEWLINE_RESTRICTED)
                && current.newlines == 0);
        start = start
            || (self.last_reserved("else")
                && !(self.reserved(Some(t), "if") && current.comments_before.is_empty()));
        start = start
            || (self.last_is(K::EndExpr)
                && (self.pf().mode == Mode::ForInitializer || self.pf().mode == Mode::Conditional));
        start = start
            || (self.last_is(K::Word)
                && self.f().mode == Mode::BlockStatement
                && !self.f().in_case
                && !(current.text == "--" || current.text == "++")
                && self.last_last_text != "function"
                && current.kind != K::Word
                && current.kind != K::Reserved);
        start = start
            || (self.f().mode == Mode::ObjectLiteral
                && ((self.last_text_is(":") && self.f().ternary_depth == 0)
                    || self.last_reserved_any(&["get", "set"])));
        if start {
            self.set_mode(Mode::Statement);
            self.indent();
            self.handle_whitespace_and_comments(t, true);
            if !self.start_of_object_property() {
                let force = self.reserved_any(Some(t), &["do", "for", "if", "while"]);
                self.allow_wrap_or_preserved_newline(t, force);
            }
            return true;
        }
        false
    }

    fn handle_start_expr(&mut self, t: usize) {
        if !self.start_of_statement(t) {
            self.handle_whitespace_and_comments(t, false);
        }
        let mut next_mode = Mode::Expression;
        if self.tokens[t].text == "[" {
            if self.last_is(K::Word) || self.last_text_is(")") {
                if self.last_reserved_any(LINE_STARTERS) {
                    self.output.space_before_token = true;
                }
                self.print_token(t);
                self.set_mode(next_mode);
                self.indent();
                return;
            }
            next_mode = Mode::ArrayLiteral;
            if is_array(self.f().mode)
                && (self.last_text_is("[")
                    || (self.last_text_is(",") && (self.last_last_text == "]" || self.last_last_text == "}")))
            {
                self.print_newline(false, false);
            }
            if !matches!(self.last().kind, K::StartExpr | K::EndExpr | K::Word | K::Operator | K::Dot) {
                self.output.space_before_token = true;
            }
        } else {
            if self.last_is(K::Reserved) {
                let text = self.last().text.clone();
                let last_word = self.f().last_word.clone();
                if text == "for" {
                    self.output.space_before_token = true;
                    next_mode = Mode::ForInitializer;
                } else if ["if", "while", "switch"].contains(&text.as_str()) {
                    self.output.space_before_token = true;
                    next_mode = Mode::Conditional;
                } else if last_word == "await" || last_word == "async" {
                    self.output.space_before_token = true;
                } else if text == "import" && self.tokens[t].whitespace_before.is_empty() {
                    self.output.space_before_token = false;
                } else if LINE_STARTERS.contains(&text.as_str()) || text == "catch" {
                    self.output.space_before_token = true;
                }
            } else if self.last_is(K::Equals) || self.last_is(K::Operator) {
                if !self.start_of_object_property() {
                    self.allow_wrap_or_preserved_newline(t, false);
                }
            } else if self.last_is(K::Word) {
                self.output.space_before_token = false;
            } else {
                self.allow_wrap_or_preserved_newline(t, false);
            }
            let last_word = self.f().last_word.clone();
            if (self.last_is(K::Reserved) && (last_word == "function" || last_word == "typeof"))
                || (self.last_text_is("*")
                    && (["function", "yield"].contains(&self.last_last_text.as_str())
                        || (self.f().mode == Mode::ObjectLiteral
                            && ["{", ","].contains(&self.last_last_text.as_str()))))
            {
                // space_after_anon_function: false
                self.output.space_before_token = false;
            }
        }
        if self.last_text_is(";") || self.last_is(K::StartBlock) {
            self.print_newline(false, false);
        } else if matches!(self.last().kind, K::EndExpr | K::StartExpr | K::EndBlock | K::Comma)
            || self.last_text_is(".")
        {
            let force = self.tokens[t].newlines > 0;
            self.allow_wrap_or_preserved_newline(t, force);
        }
        self.print_token(t);
        self.set_mode(next_mode);
        self.indent();
    }

    fn handle_end_expr(&mut self, t: usize) {
        while self.f().mode == Mode::Statement {
            self.restore_mode();
        }
        self.handle_whitespace_and_comments(t, false);
        if self.f().multiline_frame {
            let force = self.tokens[t].text == "]" && is_array(self.f().mode);
            self.allow_wrap_or_preserved_newline(t, force);
        }
        self.deindent();
        self.print_token(t);
        self.restore_mode();
        self.remove_redundant_indentation(self.previous_flags);
        if self.f().do_while && self.pf().mode == Mode::Conditional {
            let p = self.previous_flags;
            self.flags_arena[p].mode = Mode::Expression;
            self.fm().do_block = false;
            self.fm().do_while = false;
        }
    }

    fn handle_start_block(&mut self, t: usize) {
        self.handle_whitespace_and_comments(t, false);
        let next = self.peek(0);
        let second = self.peek(1);
        let next_tok = next.map(|n| self.tokens[n].clone());
        let second_tok = second.map(|n| self.tokens[n].clone());
        if self.f().last_word == "switch" && self.last_is(K::EndExpr) {
            self.set_mode(Mode::BlockStatement);
            self.fm().in_case_statement = true;
        } else if self.f().case_body {
            self.set_mode(Mode::BlockStatement);
        } else if let Some(s) = second_tok.as_ref().filter(|s| {
            next_tok.as_ref().is_some_and(|n| {
                ([":", ","].contains(&s.text.as_str()) && matches!(n.kind, K::Str | K::Word | K::Reserved))
                    || (["get", "set", "..."].contains(&n.text.as_str())
                        && matches!(s.kind, K::Word | K::Reserved))
            })
        }) {
            if ["class", "interface"].contains(&self.last_last_text.as_str())
                && ![":", ","].contains(&s.text.as_str())
            {
                self.set_mode(Mode::BlockStatement);
            } else {
                self.set_mode(Mode::ObjectLiteral);
            }
        } else if self.last_is(K::Operator) && self.last_text_is("=>") {
            self.set_mode(Mode::BlockStatement);
        } else if matches!(self.last().kind, K::Equals | K::StartExpr | K::Comma | K::Operator)
            || self.last_reserved_any(&["return", "throw", "import", "default"])
        {
            self.set_mode(Mode::ObjectLiteral);
        } else {
            self.set_mode(Mode::BlockStatement);
        }
        if self.reserved_any(self.last().previous, &["class", "extends"]) {
            self.fm().class_start_block = true;
        }
        let empty_braces = next_tok.as_ref().is_some_and(|n| n.comments_before.is_empty() && n.text == "}");
        // brace_style: collapse
        if is_array(self.pf().mode) && (self.last_is(K::StartExpr) || self.last_is(K::Comma)) {
            if self.last_is(K::Comma) {
                self.output.space_before_token = true;
                self.allow_wrap_or_preserved_newline(t, false);
                let p = self.previous_flags;
                let mf = self.f().multiline_frame;
                self.flags_arena[p].multiline_frame = self.flags_arena[p].multiline_frame || mf;
                self.fm().multiline_frame = false;
            }
        }
        if !self.last_is(K::Operator) && !self.last_is(K::StartExpr) {
            if matches!(self.last().kind, K::StartBlock | K::Semicolon) && !self.f().inline_frame {
                self.print_newline(false, false);
            } else {
                self.output.space_before_token = true;
            }
        }
        self.print_token(t);
        self.indent();
        if !empty_braces {
            self.print_newline(false, false);
        }
    }

    fn handle_end_block(&mut self, t: usize) {
        self.handle_whitespace_and_comments(t, false);
        while self.f().mode == Mode::Statement {
            self.restore_mode();
        }
        let empty_braces = self.last_is(K::StartBlock);
        if self.f().inline_frame && !empty_braces {
            self.output.space_before_token = true;
        } else if !empty_braces {
            self.print_newline(false, false);
        }
        self.restore_mode();
        self.print_token(t);
    }

    fn handle_word(&mut self, t: usize) {
        if self.tokens[t].kind == K::Reserved {
            let text = self.tokens[t].text.clone();
            if (text == "set" || text == "get") && self.f().mode != Mode::ObjectLiteral {
                self.tokens[t].kind = K::Word;
            } else if text == "import"
                && self.peek(0).is_some_and(|n| ["(", "."].contains(&self.tokens[n].text.as_str()))
            {
                self.tokens[t].kind = K::Word;
            } else if (text == "as" || text == "from") && !self.f().import_block {
                self.tokens[t].kind = K::Word;
            } else if self.f().mode == Mode::ObjectLiteral
                && self.peek(0).is_some_and(|n| self.tokens[n].text == ":")
            {
                self.tokens[t].kind = K::Word;
            }
        }
        if self.start_of_statement(t) {
            if self.last_reserved_any(&["var", "let", "const"]) && self.tokens[t].kind == K::Word {
                self.fm().declaration_statement = true;
            }
        } else if self.tokens[t].newlines > 0
            && !is_expression(self.f().mode)
            && (!self.last_is(K::Operator) || self.last_text_is("--") || self.last_text_is("++"))
            && !self.last_is(K::Equals)
        {
            self.handle_whitespace_and_comments(t, false);
            self.print_newline(false, false);
        } else {
            self.handle_whitespace_and_comments(t, false);
        }
        if self.f().do_block && !self.f().do_while {
            if self.reserved(Some(t), "while") {
                self.output.space_before_token = true;
                self.print_token(t);
                self.output.space_before_token = true;
                self.fm().do_while = true;
                return;
            } else {
                self.print_newline(false, false);
                self.fm().do_block = false;
            }
        }
        if self.f().if_block {
            if !self.f().else_block && self.reserved(Some(t), "else") {
                self.fm().else_block = true;
            } else {
                while self.f().mode == Mode::Statement {
                    self.restore_mode();
                }
                self.fm().if_block = false;
                self.fm().else_block = false;
            }
        }
        if self.f().in_case_statement && self.reserved_any(Some(t), &["case", "default"]) {
            self.print_newline(false, false);
            if !self.f().case_block && self.f().case_body {
                self.deindent();
            }
            self.fm().case_body = false;
            self.print_token(t);
            self.fm().in_case = true;
            return;
        }
        if matches!(self.last().kind, K::Comma | K::StartExpr | K::Equals | K::Operator)
            && !self.start_of_object_property()
            && !((self.last_text_is("+") || self.last_text_is("-"))
                && self.last_last_text == ":"
                && self.parent_mode() == Some(Mode::ObjectLiteral))
        {
            self.allow_wrap_or_preserved_newline(t, false);
        }
        if self.reserved(Some(t), "function") {
            let last_text = self.last().text.clone();
            if last_text == "}"
                || last_text == ";"
                || (self.output.just_added_newline()
                    && !(["(", "[", "{", ":", "=", ","].contains(&last_text.as_str())
                        || self.last_is(K::Operator)))
            {
                if !self.output.just_added_blankline() && self.tokens[t].comments_before.is_empty() {
                    self.print_newline(false, false);
                    self.print_newline(true, false);
                }
            }
            if self.last_is(K::Reserved) || self.last_is(K::Word) {
                if self.last_reserved_any(&["get", "set", "new", "export"])
                    || self.last_reserved_any(NEWLINE_RESTRICTED)
                {
                    self.output.space_before_token = true;
                } else if self.last_reserved("default") && self.last_last_text == "export" {
                    self.output.space_before_token = true;
                } else if self.last_text_is("declare") {
                    self.output.space_before_token = true;
                } else {
                    self.print_newline(false, false);
                }
            } else if self.last_is(K::Operator) || self.last_text_is("=") {
                self.output.space_before_token = true;
            } else if !self.f().multiline_frame && (is_expression(self.f().mode) || is_array(self.f().mode)) {
            } else {
                self.print_newline(false, false);
            }
            self.print_token(t);
            self.fm().last_word = self.tokens[t].text.clone();
            return;
        }
        let mut prefix = Prefix::None;
        if self.last_is(K::EndBlock) {
            if self.pf().inline_frame {
                prefix = Prefix::Space;
            } else if !self.reserved_any(Some(t), &["else", "catch", "finally", "from"]) {
                prefix = Prefix::Newline;
            } else {
                prefix = Prefix::Space;
                self.output.space_before_token = true;
            }
        } else if self.last_is(K::Semicolon) && self.f().mode == Mode::BlockStatement {
            prefix = Prefix::Newline;
        } else if self.last_is(K::Semicolon) && is_expression(self.f().mode) {
            prefix = Prefix::Space;
        } else if self.last_is(K::Str) {
            prefix = Prefix::Newline;
        } else if self.last_is(K::Reserved)
            || self.last_is(K::Word)
            || (self.last_text_is("*")
                && (["function", "yield"].contains(&self.last_last_text.as_str())
                    || (self.f().mode == Mode::ObjectLiteral
                        && ["{", ","].contains(&self.last_last_text.as_str()))))
        {
            prefix = Prefix::Space;
        } else if self.last_is(K::StartBlock) {
            prefix = if self.f().inline_frame { Prefix::Space } else { Prefix::Newline };
        } else if self.last_is(K::EndExpr) {
            self.output.space_before_token = true;
            prefix = Prefix::Newline;
        }
        if self.reserved_any(Some(t), LINE_STARTERS) && !self.last_text_is(")") {
            prefix = if self.f().inline_frame || self.last_text_is("else") || self.last_text_is("export") {
                Prefix::Space
            } else {
                Prefix::Newline
            };
        }
        if self.reserved_any(Some(t), &["else", "catch", "finally"]) {
            if !(self.last_is(K::EndBlock) && self.pf().mode == Mode::BlockStatement)
                && !self.f().inline_frame
            {
                self.print_newline(false, false);
            } else {
                self.output.trim(true);
                if self.output.current_line().last().map(String::as_str) != Some("}") {
                    self.print_newline(false, false);
                }
                self.output.space_before_token = true;
            }
        } else if prefix == Prefix::Newline {
            if self.last_reserved_any(SPECIAL_WORDS) {
                self.output.space_before_token = true;
            } else if self.last_text_is("declare") && self.reserved_any(Some(t), &["var", "let", "const"]) {
                self.output.space_before_token = true;
            } else if !self.last_is(K::EndExpr) {
                if (!self.last_is(K::StartExpr) || !self.reserved_any(Some(t), &["var", "let", "const"]))
                    && !self.last_text_is(":")
                {
                    if self.reserved(Some(t), "if") && self.reserved(self.tokens[t].previous, "else") {
                        self.output.space_before_token = true;
                    } else {
                        self.print_newline(false, false);
                    }
                }
            } else if self.reserved_any(Some(t), LINE_STARTERS) && !self.last_text_is(")") {
                self.print_newline(false, false);
            }
        } else if self.f().multiline_frame
            && is_array(self.f().mode)
            && self.last_text_is(",")
            && self.last_last_text == "}"
        {
            self.print_newline(false, false);
        } else if prefix == Prefix::Space {
            self.output.space_before_token = true;
        }
        if self.tokens[t].previous.is_some_and(|p| matches!(self.tokens[p].kind, K::Word | K::Reserved)) {
            self.output.space_before_token = true;
        }
        // Intelephense: `<?php N ?>` opens / closes PHP blocks
        let adjustment = php_adjustment(&self.tokens[t].text);
        if let Some(n) = adjustment.filter(|n| *n < 1) {
            let mut e = if n == 0 { -1 } else { n };
            while e < 0 {
                self.deindent();
                e += 1;
            }
        }
        self.print_token(t);
        if let Some(n) = adjustment.filter(|n| *n > -1) {
            let mut e = if n == 0 { 1 } else { n };
            while e > 0 {
                self.indent();
                e -= 1;
            }
        }
        self.fm().last_word = self.tokens[t].text.clone();
        if self.tokens[t].kind == K::Reserved {
            let text = self.tokens[t].text.clone();
            if text == "do" {
                self.fm().do_block = true;
            } else if text == "if" {
                self.fm().if_block = true;
            } else if text == "import" {
                self.fm().import_block = true;
            } else if self.f().import_block && text == "from" {
                self.fm().import_block = false;
            }
        }
    }

    fn handle_semicolon(&mut self, t: usize) {
        if self.start_of_statement(t) {
            self.output.space_before_token = false;
        } else {
            self.handle_whitespace_and_comments(t, false);
        }
        let next = self.peek(0);
        while self.f().mode == Mode::Statement
            && !(self.f().if_block && self.reserved(next, "else"))
            && !self.f().do_block
        {
            self.restore_mode();
        }
        if self.f().import_block {
            self.fm().import_block = false;
        }
        self.print_token(t);
    }

    fn handle_string(&mut self, t: usize) {
        let tok = self.tokens[t].clone();
        let prev_text = tok.previous.map(|p| self.tokens[p].text.clone()).unwrap_or_default();
        if tok.text.starts_with('`')
            && tok.newlines == 0
            && tok.whitespace_before.is_empty()
            && (prev_text == ")" || self.last_is(K::Word))
        {
        } else if self.start_of_statement(t) {
            self.output.space_before_token = true;
        } else {
            self.handle_whitespace_and_comments(t, false);
            if self.last_is(K::Reserved) || self.last_is(K::Word) || self.f().inline_frame {
                self.output.space_before_token = true;
            } else if matches!(self.last().kind, K::Comma | K::StartExpr | K::Equals | K::Operator) {
                if !self.start_of_object_property() {
                    self.allow_wrap_or_preserved_newline(t, false);
                }
            } else if tok.text.starts_with('`')
                && self.last_is(K::EndExpr)
                && (prev_text == "]" || prev_text == ")")
                && tok.newlines == 0
            {
                self.output.space_before_token = true;
            } else {
                self.print_newline(false, false);
            }
        }
        self.print_token(t);
    }

    fn handle_equals(&mut self, t: usize) {
        if !self.start_of_statement(t) {
            self.handle_whitespace_and_comments(t, false);
        }
        if self.f().declaration_statement {
            self.fm().declaration_assignment = true;
        }
        self.output.space_before_token = true;
        self.print_token(t);
        self.output.space_before_token = true;
    }

    fn handle_comma(&mut self, t: usize) {
        self.handle_whitespace_and_comments(t, true);
        self.print_token(t);
        self.output.space_before_token = true;
        if self.f().declaration_statement {
            if self.parent_mode().is_some_and(is_expression) {
                self.fm().declaration_assignment = false;
            }
            if self.f().declaration_assignment {
                self.fm().declaration_assignment = false;
                self.print_newline(false, true);
            }
        } else if self.f().mode == Mode::ObjectLiteral
            || (self.f().mode == Mode::Statement && self.parent_mode() == Some(Mode::ObjectLiteral))
        {
            if self.f().mode == Mode::Statement {
                self.restore_mode();
            }
            if !self.f().inline_frame {
                self.print_newline(false, false);
            }
        }
    }

    fn handle_operator(&mut self, t: usize) {
        let text = self.tokens[t].text.clone();
        let is_generator_asterisk = text == "*"
            && (self.last_reserved_any(&["function", "yield"])
                || matches!(self.last().kind, K::StartBlock | K::Comma | K::EndBlock | K::Semicolon));
        let is_unary = (text == "-" || text == "+")
            && (matches!(self.last().kind, K::StartBlock | K::StartExpr | K::Equals | K::Operator)
                || LINE_STARTERS.contains(&self.last().text.as_str())
                || self.last_text_is(","));
        if !self.start_of_statement(t) {
            let preserve = !is_generator_asterisk;
            self.handle_whitespace_and_comments(t, preserve);
        }
        if text == "*" && self.last_is(K::Dot) {
            self.print_token(t);
            return;
        }
        if text == "::" {
            self.print_token(t);
            return;
        }
        if (text == "-" || text == "+") && self.start_of_object_property() {
            self.print_token(t);
            return;
        }
        if self.last_is(K::Operator) {
            // operator_position: before-newline
            self.allow_wrap_or_preserved_newline(t, false);
        }
        if text == ":" && self.f().in_case {
            self.print_token(t);
            self.fm().in_case = false;
            self.fm().case_body = true;
            if self.peek(0).is_none_or(|n| self.tokens[n].kind != K::StartBlock) {
                self.indent();
                self.print_newline(false, false);
                self.fm().case_block = false;
            } else {
                self.fm().case_block = true;
                self.output.space_before_token = true;
            }
            return;
        }
        let mut space_before = true;
        let mut space_after = true;
        let mut in_ternary = false;
        if text == ":" {
            if self.f().ternary_depth == 0 {
                space_before = false;
            } else {
                self.fm().ternary_depth -= 1;
                in_ternary = true;
            }
        } else if text == "?" {
            self.fm().ternary_depth += 1;
        }
        if !is_unary
            && !is_generator_asterisk
            && self.options.base.preserve_newlines
            && POSITIONABLE_OPERATORS.contains(&text.as_str())
        {
            // operator_position: before-newline
            let is_colon = text == ":";
            let is_ternary_colon = is_colon && in_ternary;
            let is_other_colon = is_colon && !in_ternary;
            self.output.space_before_token = !is_other_colon;
            self.print_token(t);
            if !is_colon || is_ternary_colon {
                self.allow_wrap_or_preserved_newline(t, false);
            }
            self.output.space_before_token = true;
            return;
        }
        if is_generator_asterisk {
            self.allow_wrap_or_preserved_newline(t, false);
            space_before = false;
            space_after = self.peek(0).is_some_and(|n| matches!(self.tokens[n].kind, K::Word | K::Reserved));
        } else if text == "..." {
            self.allow_wrap_or_preserved_newline(t, false);
            space_before = self.last_is(K::StartBlock);
            space_after = false;
        } else if ["--", "++", "!", "~"].contains(&text.as_str()) || is_unary {
            if self.last_is(K::Comma) || self.last_is(K::StartExpr) {
                self.allow_wrap_or_preserved_newline(t, false);
            }
            space_before = false;
            space_after = false;
            let newlines = self.tokens[t].newlines;
            if newlines > 0 && (text == "--" || text == "++" || text == "~") {
                let new_line_needed = self.last_reserved_any(SPECIAL_WORDS) && newlines > 0;
                if new_line_needed && (self.pf().if_block || self.pf().else_block) {
                    self.restore_mode();
                }
                self.print_newline(new_line_needed, true);
            }
            if self.last_text_is(";") && is_expression(self.f().mode) {
                space_before = true;
            }
            if self.last_is(K::Reserved) {
                space_before = true;
            } else if self.last_is(K::EndExpr) {
                space_before = !(self.last_text_is("]") && (text == "--" || text == "++"));
            } else if self.last_is(K::Operator) {
                let ops = ["--", "-", "++", "+"];
                space_before = ops.contains(&text.as_str()) && ops.contains(&self.last().text.as_str());
                if (text == "+" || text == "-") && (self.last_text_is("--") || self.last_text_is("++")) {
                    space_after = true;
                }
            }
            if ((self.f().mode == Mode::BlockStatement && !self.f().inline_frame)
                || self.f().mode == Mode::Statement)
                && (self.last_text_is("{") || self.last_text_is(";"))
            {
                self.print_newline(false, false);
            }
        }
        self.output.space_before_token = self.output.space_before_token || space_before;
        self.print_token(t);
        self.output.space_before_token = space_after;
    }

    fn handle_block_comment(&mut self, t: usize, preserve: bool) {
        if self.output.raw {
            let tok = self.tokens[t].clone();
            self.output.add_raw_token(tok.newlines, &tok.whitespace_before, &tok.text);
            if tok.directive("preserve") == Some("end") {
                self.output.raw = false;
            }
            return;
        }
        if self.tokens[t].directives.is_some() {
            self.print_newline(false, preserve);
            self.print_token(t);
            if self.tokens[t].directive("preserve") == Some("start") {
                self.output.raw = true;
            }
            self.print_newline(false, true);
            return;
        }
        if !self.tokens[t].text.contains(is_newline) && self.tokens[t].newlines == 0 {
            self.output.space_before_token = true;
            self.print_token(t);
            self.output.space_before_token = true;
        } else {
            self.print_block_comment(t, preserve);
        }
    }

    fn print_block_comment(&mut self, t: usize, preserve: bool) {
        let tok = self.tokens[t].clone();
        let mut lines = split_linebreaks(&tok.text);
        let last_indent = tok.whitespace_before.clone();
        self.print_newline(false, preserve);
        self.print_token_line_indentation();
        self.output.add_token(lines.first().map_or("", String::as_str));
        self.print_newline(false, preserve);
        if lines.len() > 1 {
            lines.remove(0);
            let javadoc = lines.iter().all(|l| l.trim().starts_with('*'));
            let starless = lines.iter().all(|l| l.is_empty() || l.starts_with(&last_indent));
            if javadoc {
                self.fm().alignment = 1;
            }
            for line in &lines {
                if javadoc {
                    self.print_token_line_indentation();
                    self.output.add_token(line.trim_start());
                } else if starless && !line.is_empty() {
                    self.print_token_line_indentation();
                    self.output.add_token(&line[last_indent.len()..]);
                } else {
                    self.output.set_current_line_unindented();
                    self.output.add_token(line);
                }
                self.print_newline(false, preserve);
            }
            self.fm().alignment = 0;
        }
    }

    fn handle_comment(&mut self, t: usize, preserve: bool) {
        if self.tokens[t].newlines > 0 {
            self.print_newline(false, preserve);
        } else {
            self.output.trim(true);
        }
        self.output.space_before_token = true;
        self.print_token(t);
        self.print_newline(false, preserve);
    }

    fn handle_dot(&mut self, t: usize) {
        if !self.start_of_statement(t) {
            self.handle_whitespace_and_comments(t, true);
        }
        let last_text = self.last().text.clone();
        if !last_text.is_empty() && last_text.bytes().all(|b| b.is_ascii_digit()) {
            self.output.space_before_token = true;
        }
        if self.last_reserved_any(SPECIAL_WORDS) {
            self.output.space_before_token = false;
        } else {
            // break_chained_methods: false
            self.allow_wrap_or_preserved_newline(t, false);
        }
        self.print_token(t);
    }

    fn handle_unknown(&mut self, t: usize, preserve: bool) {
        self.print_token(t);
        if self.tokens[t].text.ends_with('\n') {
            self.print_newline(false, preserve);
        }
    }

    fn handle_eof(&mut self, t: usize) {
        while self.f().mode == Mode::Statement {
            self.restore_mode();
        }
        self.handle_whitespace_and_comments(t, false);
    }
}

fn split_linebreaks(s: &str) -> Vec<String> {
    let s = normalize_line_breaks(s);
    let mut out: Vec<String> = Vec::new();
    let mut rest = s.as_str();
    while let Some(i) = rest.find('\n') {
        out.push(rest[..i].to_string());
        rest = &rest[i + 1..];
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

/// Formats JavaScript like js-beautify with the options a `<script>` in a
/// template gets from Intelephense.
pub(crate) fn beautify(text: &str, options: &ChildOptions) -> String {
    let mut base = BaseOptions::new(options.indent_size, options.use_tabs);
    base.wrap_line_length = options.wrap_line_length;
    base.templating = vec!["php".into()];
    Beautifier::beautify(text, Options { base })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn js(s: &str) -> String {
        beautify(s, &ChildOptions { indent_size: 4, use_tabs: false, wrap_line_length: 0 })
    }

    #[test]
    fn matches_js_beautify() {
        let input = "jQuery(function($){var o={a:1,'b':[1,2,{c:3}],d:function(x){return x?1:2}};\n/* block\n * comment */\n$('#x').on('click',function(e){e.preventDefault();// line\nif(!o.a){o.d(1)}else if(o.b){for(var i=0;i<3;i++){console.log(`t ${i}`)}}\nswitch(e.which){case 1:break;default:o.a=/re[/]x/g.test(\"s\")}});});\n";
        let expected = "jQuery(function($) {\n    var o = {\n        a: 1,\n        'b': [1, 2, {\n            c: 3\n        }],\n        d: function(x) {\n            return x ? 1 : 2\n        }\n    };\n    /* block\n     * comment */\n    $('#x').on('click', function(e) {\n        e.preventDefault(); // line\n        if (!o.a) {\n            o.d(1)\n        } else if (o.b) {\n            for (var i = 0; i < 3; i++) {\n                console.log(`t ${i}`)\n            }\n        }\n        switch (e.which) {\n            case 1:\n                break;\n            default:\n                o.a = /re[/]x/g.test(\"s\")\n        }\n    });\n});";
        assert_eq!(js(input), expected);
    }

    #[test]
    fn php_placeholders_indent() {
        let input = "var a = 1;\n<?php 1 ?>\nif (a) {\nb();\n}\n<?php -1 ?>\nfoo(<?php echo $x; ?>);\n<?php 0 ?>\nc();\n";
        let expected = "var a = 1;\n<?php 1 ?>\n    if (a) {\n        b();\n    }\n<?php -1 ?>\nfoo(<?php echo $x; ?>);\n<?php 0 ?>\n    c();";
        assert_eq!(js(input), expected);
    }

    #[test]
    fn formats_statements() {
        assert_eq!(js("var a=1;if(a){b()}else{c()}"), "var a = 1;\nif (a) {\n    b()\n} else {\n    c()\n}");
    }
}
