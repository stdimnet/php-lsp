# Diagnostics

php-lsp reports the 131 diagnostic codes of Intelephense's catalog with the same codes, messages, ranges and severities (checked case by case against the real Intelephense for PHP 7.4 to 8.5). The table shows one message per code; `…` stands for a name, type or number filled in at runtime.

Diagnostics can be switched off by group with the `diagnostics.*` settings (see [configuration](configuration.md#settings)), or all at once with `diagnostics.enable: false`. External analysers (PHPStan, Psalm, PHPCS) add their own diagnostics, see [configuration](configuration.md#external-tools).

| Code | Message |
|---|---|
| `P1001` | Syntax error (unexpected or missing token). |
| `P1003` | Symbol 'Some\Unused' is declared but not used. |
| `P1004` | '…' is already declared (duplicate declaration). |
| `P1005` | Not enough arguments. Expected 2. Found 1. |
| `P1006` | Expected type 'Shape'. Found 'Other'. |
| `P1007` | 'Old\Cache' is deprecated. use New |
| `P1008` | Undefined variable '$undefinedVar'. |
| `P1009` | Undefined type 'App\Nope'. |
| `P1010` | Undefined function 'App\undefined_fn'. |
| `P1011` | Undefined constant '…'. |
| `P1012` | Undefined class constant 'NOPE'. |
| `P1013` | Undefined method 'missing'. |
| `P1014` | Undefined property '$nope'. |
| `P1015` | Multiple visibility modifiers found. |
| `P1016` | Final modifier not allowed. |
| `P1017` | Abstract methods cannot be private. |
| `P1018` | Abstract modifier not allowed. |
| `P1019` | Interface methods must be public. |
| `P1020` | Method body expected. |
| `P1021` | Method body not allowed. |
| `P1022` | PHP 8.5+ syntax found. Targeting PHP 8.4. |
| `P1023` | 'Iterable' is a reserved word and cannot be used to name a class, interface, trait or namespace. |
| `P1024` | Expression is not writable. |
| `P1025` | Empty array destructure. |
| `P1026` | Key expected in read context. |
| `P1027` | Unexpected indentation level. Expected at least 2. Found 1. |
| `P1028` | Invalid indentation. Mixed spaces and tabs are not allowed. |
| `P1029` | Abstract method in non abstract class. |
| `P1030` | Cannot use '$this' in non-object context. |
| `P1031` | '(real)' cast deprecated in PHP 7.4. Targeting PHP 8.5. |
| `P1032` | '(unset)' cast deprecated in PHP 7.2. Targeting PHP 8.5. |
| `P1033` | Nested ternary expressions (without parentheses) deprecated in PHP 7.4. Targeting PHP 8.5. |
| `P1034` | Array or string offset access with curly braces deprecated in PHP 7.4. Targeting PHP 8.5. |
| `P1035` | Cannot use 'parent' in a class with no parent. |
| `P1037` | 'App\Circle' does not implement methods App\Base::name, App\Shape::area. |
| `P1038` | Method 'Child::a()' is not compatible with method 'Base::a()'. |
| `P1039` | Method '…::…()' cannot override final method '…::…()'. |
| `P1040` | Namespace names can only contain keywords in PHP 8+. |
| `P1041` | Cannot declare property promotion outside of constructor. |
| `P1042` | Cannot use positional argument after named argument. |
| `P1043` | Named argument … overwrites previous argument. |
| `P1044` | Unknown named argument …. |
| `P1045` | Cannot use 'readonly' as method modifier. |
| `P1046` | Readonly property must have a type. |
| `P1047` | Readonly property cannot have a default value. |
| `P1048` | Type 'void' cannot be used in this context. |
| `P1049` | Enum case can only be used in an enum. |
| `P1050` | Case … of backed enum … must have a value. |
| `P1051` | Enum backing type must be int or string. |
| `P1052` | Case X of non-backed enum Pure must not have a value. |
| `P1053` | Cannot create closure for new expression. |
| `P1054` | Cannot combine nullsafe operator with closure creation. |
| `P1055` | Traits cannot have constants. |
| `P1057` | Static property cannot be readonly. |
| `P1058` | Readonly class cannot declare static properties. |
| `P1059` | Non-readonly class cannot extend readonly class. |
| `P1060` | Readonly class cannot extend non-readonly class. |
| `P1061` | Cannot redeclare readonly property as non-readonly. |
| `P1062` | Cannot redeclare readonly property as non-readonly. |
| `P1063` | Using … in strings is deprecated. |
| `P1064` | 'null' cannot be used as a standalone type. |
| `P1065` | Redundant type. |
| `P1066` | Cannot use private(set), protected(set), public(set) on non-property. |
| `P1067` | A property with asymmetric visibility must have a type. |
| `P1068` | Set visibility must be equal to or less than get visibility. |
| `P1069` | Static property cannot have asymmetric visibility. |
| `P1070` | Visibility of 'Vh::$p' must be the same or less restrictive than 'Vg::$p'. |
| `P1071` | Cannot specify set visibility for a get only virtual property $x. |
| `P1072` | Virtual property $v cannot have a default value. |
| `P1073` | Cannot indirectly modify property Rg::$a. |
| `P1074` | Invalid property hook signature. |
| `P1075` | Not all paths return a value. |
| `P1076` | Cannot unset hooked property HU::$x. |
| `P1077` | Property Td::$p must have type null\|int. |
| `P1078` | Implicitly nullable parameters are deprecated. |
| `P1079` | Hooked properties cannot be readonly. |
| `P1080` | Cannot access private method B::hidden from global scope. |
| `P1081` | Static modifier not allowed. |
| `P1082` | Interfaces may only contain hooked properties. |
| `P1083` | Case statements followed by a semicolon (;) are deprecated, use a colon (:) instead. |
| `P1084` | Non-canonical cast (binary) is deprecated. Use (string) instead. |
| `P1085` | The backtick (`) operator is deprecated, use shell_exec() instead |
| `P1086` | Attribute class '…' is not an attribute (missing `#[Attribute]`). |
| `P1087` | Attribute "OnlyClass" has invalid target. |
| `P1088` | Attribute '…' is not repeatable. |
| `P1089` | Constant expression contains invalid operations. |
| `P1090` | Closures in constant expressions must be static. |
| `P1091` | Closures in constant expressions must be static. |
| `P1092` | Arrow functions on the right hand side of \|> must be parenthesized. |
| `P1093` | Type … has colliding trait methods …. |
| `P1094` | Class Bad cannot extend final class Locked. |
| `P1095` | Duplicate type Dc in extends list. |
| `P1096` | Enum … cannot implement magic method …. |
| `P1097` | Enum … cannot include properties. |
| `P1098` | Cannot instantiate interface App\Shape. |
| `P1099` | Defining a custom assert() function is deprecated. |
| `P1100` | Defining a function named "assert" is not allowed. |
| `P1101` | Optional parameter declared before required parameter is implicitly treated as required. |
| `P1102` | Class constant K2::A is not compatible with K1::A. |
| `P1103` | Duplicate enum case value …. |
| `P1104` | Break and continue can only be used in loop or switch context. |
| `P1105` | Namespace declaration must be the first statement in script or immediately after declare statement. |
| `P1106` | Cannot mix bracketed and unbracketed namespace declarations. |
| `P1107` | No code may exist outside of bracketed namespace. |
| `P1108` | Unreachable code detected. |
| `P1109` | Method '__construct' cannot declare a return type. |
| `P1110` | In the composition of type 'CP', properties 'CP::$x' and 'TP::$x' are not compatible. |
| `P1111` | In the composition of type 'Cc', class constants 'Ca::X' and 'Cb::X' are not compatible. |
| `P1112` | In the composition of type 'Mc', methods 'Ma::m()' and 'Mb::m()' are not compatible. |
| `P1113` | A variadic parameter cannot have a default argument. |
| `P1114` | Only variables should be passed by reference. |
| `P1115` | Cannot use [] operator on string. |
| `P1116` | Possible undefined variable '$x'. |
| `P1117` | (void) cast cannot be used in this context. |
| `P1118` | Duplicate array key …. |
| `P1119` | Too many arguments. Expected 2. Found 3. |
| `P1120` | Only the last parameter can be variadic. |
| `P1121` | Incorrect string concatenation operator. |
| `P1122` | Possible unintended assignment in conditional expression. |
| `P1123` | Duplicate branch expression …. |
| `P1124` | Loop variable $item is also declared as a parameter. |
| `P1125` | Assigning return type of 'void'. |
| `P1126` | The return value of nd() is expected to be consumed. |
| `P1127` | Attribute 'NoDiscard' cannot be used on magic method '__construct'. |
| `P1128` | OC::b() has #[\Override] attribute, but no matching parent method exists. |
| `P1129` | PHPDoc parse error. Unexpected token. |
| `P1130` | Unknown @param parameter name '…'. |
| `P1131` | Documented type is not compatible with the declared type. |
| `P1132` | Parameter $v has no type information available. |
| `P1133` | PHPDoc undefined type '…'. |
| `P1134` | 'Ca' has a circular reference. |

## How php-lsp keeps false positives down

- Undefined members are not reported on classes with unknown ancestors (e.g. a `vendor/` directory that is not installed) or on interfaces that may have more implementations.
- Guards are respected: `method_exists()`, `property_exists()`, `function_exists()`, `class_exists()`, `isset()`, `??`, `instanceof` and `is_*()` checks, also in early returns.
- Closures that may be rebound (`Closure::bind`, `->call()`) are not checked against `$this`.
- Correlated conditions (`if ($a) $x = 1; ... if ($a) echo $x;`), `switch` with `default` and `while (true)` are understood by the undefined-variable and missing-return checks.
- Argument type checks are relaxed like Intelephense: unions, scalar coercion without `strict_types` and unknown classes are accepted.
