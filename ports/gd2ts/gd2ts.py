#!/usr/bin/env python3
"""gd2ts — a GDScript -> ESM JavaScript transpiler for the disciplined, typed
subset the Axiom Godot ports are written in (typed classes, vectors, arithmetic,
control flow). Phase 1 target: the pure sim. Emits modules that `import * as gd`
from runtime.mjs.

    python gd2ts.py scripts/math_util.gd > out/math_util.mjs

Two real semantic gaps are handled with light, annotation-driven type tracking:
  * GDScript ints are 64-bit; JS numbers aren't. Integer `*` -> Math.imul and
    bitwise/shift results are normalized with `>>> 0`, so the 32-bit hash math
    (hash01) stays bit-identical.
  * No operator overloading in JS: Vector +/-/* on typed vectors -> .add/.sub/.mul.
"""
import re
import sys

KEYWORDS = {"func", "static", "const", "var", "if", "elif", "else", "for", "while",
            "return", "match", "pass", "break", "continue", "in", "and", "or", "not",
            "true", "false", "null", "self", "extends", "class_name", "enum", "as", "range"}

TYPE_CTORS = {"Vector2", "Vector3", "Quaternion", "Color", "Transform3D", "Basis", "Projection"}
GLOBAL_FUNCS = {"cos", "sin", "tan", "sqrt", "exp", "atan2", "floor", "ceil", "absf",
                "minf", "maxf", "fmod", "clampf", "clamp", "roundi", "mini", "maxi",
                "clampi", "absi", "fposmod", "snappedf", "sign", "signf"}
INT_FUNCS = {"roundi", "mini", "maxi", "clampi", "absi", "floori", "ceili"}
FLOAT_FUNCS = {"cos", "sin", "tan", "sqrt", "exp", "atan2", "floor", "ceil", "absf",
               "minf", "maxf", "fmod", "clampf", "clamp", "sign", "signf"}


# ── lexer (tab-indent aware) ────────────────────────────────────────────────────
class Tok:
    def __init__(self, kind, val):
        self.kind = kind
        self.val = val

    def __repr__(self):
        return f"{self.kind}:{self.val!r}"


TOKEN_RE = re.compile(r"""
    (?P<hex>0x[0-9a-fA-F_]+)
  | (?P<float>\d[\d_]*\.\d[\d_]*|\d[\d_]*\.|\.\d[\d_]*)
  | (?P<int>\d[\d_]*)
  | (?P<str>"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')
  | (?P<name>[A-Za-z_][A-Za-z_0-9]*)
  | (?P<op>->|:=|==|!=|<=|>=|<<|>>|\+=|-=|\*=|/=|%=|&=|\|=|\^=|&&|\|\||[-+*/%<>=(){}\[\],.:&|\^~!@])
""", re.VERBOSE)


def lex(src):
    lines = src.split("\n")
    toks = []
    indents = [0]
    bracket = 0  # () [] {} depth — continuation lines inside brackets ignore newlines/indent
    for raw in lines:
        line = strip_comment(raw)
        if line.strip() == "":
            continue
        col = len(line) - len(line.lstrip("\t"))
        if bracket == 0:
            # indentation only matters at statement start (depth 0)
            if col > indents[-1]:
                indents.append(col)
                toks.append(Tok("INDENT", None))
            while col < indents[-1]:
                indents.pop()
                toks.append(Tok("DEDENT", None))
        i = col
        s = line
        while i < len(s):
            if s[i] in " \t":
                i += 1
                continue
            m = TOKEN_RE.match(s, i)
            if not m:
                raise SyntaxError(f"cannot lex: {s[i:]!r}")
            kind = m.lastgroup
            val = m.group()
            i = m.end()
            if val in "([{":
                bracket += 1
            elif val in ")]}":
                bracket = max(0, bracket - 1)
            if kind == "name" and val in KEYWORDS:
                toks.append(Tok("kw", val))
            else:
                toks.append(Tok(kind, val))
        if bracket == 0:
            toks.append(Tok("NEWLINE", None))
    while len(indents) > 1:
        indents.pop()
        toks.append(Tok("DEDENT", None))
    toks.append(Tok("EOF", None))
    return toks


def strip_comment(line):
    out = []
    instr = None
    i = 0
    while i < len(line):
        c = line[i]
        if instr:
            out.append(c)
            if c == "\\" and i + 1 < len(line):
                out.append(line[i + 1]); i += 2; continue
            if c == instr:
                instr = None
        elif c in "\"'":
            instr = c; out.append(c)
        elif c == "#":
            break
        else:
            out.append(c)
        i += 1
    return "".join(out)


# ── parser (Pratt) ──────────────────────────────────────────────────────────────
class Parser:
    def __init__(self, toks):
        self.toks = toks
        self.i = 0

    def peek(self, k=0):
        return self.toks[self.i + k]

    def next(self):
        t = self.toks[self.i]
        self.i += 1
        return t

    def accept(self, kind, val=None):
        t = self.peek()
        if t.kind == kind and (val is None or t.val == val):
            return self.next()
        return None

    def expect(self, kind, val=None):
        t = self.accept(kind, val)
        if not t:
            raise SyntaxError(f"expected {kind} {val}, got {self.peek()}")
        return t

    def skip_newlines(self):
        while self.peek().kind == "NEWLINE":
            self.next()

    def parse_module(self):
        decls = []
        self.skip_newlines()
        while self.peek().kind != "EOF":
            decls.append(self.parse_toplevel())
            self.skip_newlines()
        return decls

    def parse_toplevel(self):
        t = self.peek()
        if t.kind == "kw" and t.val in ("extends", "class_name"):
            self.next(); self.next(); self.expect("NEWLINE")
            return ("skip",)
        if t.kind == "kw" and t.val == "const":
            return self.parse_const()
        if t.kind == "kw" and t.val in ("static", "func"):
            return self.parse_func()
        if t.kind == "kw" and t.val == "var":
            return self.parse_field()
        raise SyntaxError(f"unexpected top-level {t}")

    def parse_const(self):
        self.expect("kw", "const")
        name = self.expect("name").val
        # optional : Type
        if self.accept("op", ":") and self.peek().kind == "name":
            self.next()
        self.accept("op", ":=") or self.expect("op", "=")
        val = self.parse_expr()
        self.expect("NEWLINE")
        return ("const", name, val)

    def parse_field(self):
        self.expect("kw", "var")
        name = self.expect("name").val
        typ = None
        if self.accept("op", ":"):
            if self.peek().kind == "name":
                typ = self.next().val
        val = None
        if self.accept("op", ":=") or self.accept("op", "="):
            val = self.parse_expr()
        self.expect("NEWLINE")
        return ("field", name, typ, val)

    def parse_func(self):
        is_static = bool(self.accept("kw", "static"))
        self.expect("kw", "func")
        name = self.expect("name").val
        self.expect("op", "(")
        params = []
        while not self.accept("op", ")"):
            pname = self.expect("name").val
            ptyp = None
            if self.accept("op", ":") and self.peek().kind == "name":
                ptyp = self.next().val
            default = None
            if self.accept("op", "="):
                default = self.parse_expr()
            params.append((pname, ptyp, default))
            self.accept("op", ",")
        ret = None
        if self.accept("op", "->"):
            ret = self.expect("name").val
        self.expect("op", ":")
        body = self.parse_block()
        return ("func", is_static, name, params, ret, body)

    def parse_block(self):
        self.expect("NEWLINE")
        self.expect("INDENT")
        stmts = []
        while self.peek().kind != "DEDENT":
            self.skip_newlines()
            if self.peek().kind == "DEDENT":
                break
            stmts.append(self.parse_stmt())
        self.expect("DEDENT")
        return stmts

    def parse_stmt(self):
        t = self.peek()
        if t.kind == "kw":
            if t.val == "var":
                return self.parse_var()
            if t.val == "return":
                self.next()
                if self.peek().kind == "NEWLINE":
                    self.expect("NEWLINE"); return ("return", None)
                e = self.parse_expr(); self.expect("NEWLINE"); return ("return", e)
            if t.val == "if":
                return self.parse_if()
            if t.val == "for":
                return self.parse_for()
            if t.val == "while":
                self.next(); c = self.parse_expr(); self.expect("op", ":"); b = self.parse_block(); return ("while", c, b)
            if t.val == "match":
                return self.parse_match()
            if t.val in ("pass", "break", "continue"):
                self.next(); self.expect("NEWLINE"); return (t.val,)
        # assignment or expression
        e = self.parse_expr()
        aug = None
        for a in ("=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^="):
            if self.peek().kind == "op" and self.peek().val == a:
                aug = self.next().val; break
        if aug:
            rhs = self.parse_expr(); self.expect("NEWLINE"); return ("assign", aug, e, rhs)
        self.expect("NEWLINE")
        return ("expr", e)

    def parse_var(self):
        self.expect("kw", "var")
        name = self.expect("name").val
        typ = None
        if self.accept("op", ":"):
            if self.peek().kind == "name":
                typ = self.next().val
        val = None
        if self.accept("op", ":=") or self.accept("op", "="):
            val = self.parse_expr()
        self.expect("NEWLINE")
        return ("var", name, typ, val)

    def parse_if(self):
        self.expect("kw", "if")
        cond = self.parse_expr(); self.expect("op", ":")
        body = self.parse_block()
        elifs = []
        els = None
        while True:
            self.skip_newlines()
            if self.accept("kw", "elif"):
                c = self.parse_expr(); self.expect("op", ":"); b = self.parse_block(); elifs.append((c, b))
            elif self.accept("kw", "else"):
                self.expect("op", ":"); els = self.parse_block()
                break
            else:
                break
        return ("if", cond, body, elifs, els)

    def parse_for(self):
        self.expect("kw", "for")
        name = self.expect("name").val
        if self.accept("op", ":") and self.peek().kind == "name":
            self.next()  # typed loop var
        self.expect("kw", "in")
        it = self.parse_expr()
        self.expect("op", ":")
        body = self.parse_block()
        return ("for", name, it, body)

    def parse_match(self):
        self.expect("kw", "match")
        subj = self.parse_expr(); self.expect("op", ":")
        self.expect("NEWLINE"); self.expect("INDENT")
        cases = []
        while self.peek().kind != "DEDENT":
            self.skip_newlines()
            if self.peek().kind == "DEDENT":
                break
            pats = [self.parse_expr()]
            while self.accept("op", ","):
                pats.append(self.parse_expr())
            self.expect("op", ":")
            body = self.parse_block()
            cases.append((pats, body))
        self.expect("DEDENT")
        return ("match", subj, cases)

    # expressions
    def parse_expr(self):
        return self.parse_ternary()

    def parse_ternary(self):
        e = self.parse_or()
        if self.peek().kind == "kw" and self.peek().val == "if":
            self.next()
            cond = self.parse_or()
            self.expect("kw", "else")
            alt = self.parse_ternary()
            return ("ternary", cond, e, alt)
        return e

    def _binleft(self, sub, ops):
        e = sub()
        while (self.peek().kind in ("op", "kw")) and self.peek().val in ops:
            op = self.next().val
            r = sub()
            e = ("bin", op, e, r)
        return e

    def parse_or(self): return self._binleft(self.parse_and, ("or",))
    def parse_and(self): return self._binleft(self.parse_not, ("and",))

    def parse_not(self):
        if self.peek().kind == "kw" and self.peek().val == "not":
            self.next(); return ("unary", "not", self.parse_not())
        return self.parse_cmp()

    def parse_cmp(self): return self._binleft(self.parse_bor, ("==", "!=", "<", ">", "<=", ">=", "in"))
    def parse_bor(self): return self._binleft(self.parse_bxor, ("|",))
    def parse_bxor(self): return self._binleft(self.parse_band, ("^",))
    def parse_band(self): return self._binleft(self.parse_shift, ("&",))
    def parse_shift(self): return self._binleft(self.parse_add, ("<<", ">>"))
    def parse_add(self): return self._binleft(self.parse_mul, ("+", "-"))
    def parse_mul(self): return self._binleft(self.parse_unary, ("*", "/", "%"))

    def parse_unary(self):
        if self.peek().kind == "op" and self.peek().val in ("-", "~", "+"):
            op = self.next().val; return ("unary", op, self.parse_unary())
        return self.parse_postfix()

    def parse_postfix(self):
        e = self.parse_primary()
        while True:
            if self.accept("op", "("):
                args = []
                while not self.accept("op", ")"):
                    args.append(self.parse_expr())
                    self.accept("op", ",")
                e = ("call", e, args)
            elif self.accept("op", "["):
                idx = self.parse_expr(); self.expect("op", "]")
                e = ("index", e, idx)
            elif self.accept("op", "."):
                name = self.expect("name").val
                e = ("member", e, name)
            else:
                break
        return e

    def parse_primary(self):
        t = self.peek()
        if t.kind in ("int", "hex", "float", "str"):
            self.next(); return ("lit", t.kind, t.val)
        if t.kind == "kw" and t.val in ("true", "false", "null", "self"):
            self.next(); return ("kw", t.val)
        if t.kind == "kw" and t.val == "range":
            self.next(); return ("name", "range")
        if t.kind == "name":
            self.next(); return ("name", t.val)
        if self.accept("op", "("):
            e = self.parse_expr(); self.expect("op", ")"); return ("paren", e)
        if self.accept("op", "["):
            items = []
            while not self.accept("op", "]"):
                items.append(self.parse_expr()); self.accept("op", ",")
            return ("array", items)
        if self.accept("op", "{"):
            pairs = []
            while not self.accept("op", "}"):
                k = self.parse_expr(); self.expect("op", ":"); v = self.parse_expr()
                pairs.append((k, v)); self.accept("op", ",")
            return ("dict", pairs)
        raise SyntaxError(f"unexpected primary {t}")


# ── type inference (light, annotation-driven) ───────────────────────────────────
VEC = {"vec2", "vec3", "quat", "color"}


class Types:
    def __init__(self, funcret):
        self.env = {}
        self.funcret = funcret

    def norm(self, t):
        if t is None:
            return "num"
        return {"int": "int", "float": "float", "bool": "bool", "String": "str",
                "Vector2": "vec2", "Vector3": "vec3", "Quaternion": "quat",
                "Color": "color", "Array": "array", "Dictionary": "dict"}.get(t, "other")

    def kind(self, n):
        k = n[0]
        if k == "lit":
            if n[1] == "float":
                return "float"
            if n[1] in ("int", "hex"):
                return "int"
            return "str"
        if k == "kw":
            return "bool" if n[1] in ("true", "false") else "other"
        if k == "name":
            return self.env.get(n[1], "num")
        if k == "paren":
            return self.kind(n[1])
        if k == "unary":
            return "bool" if n[1] == "not" else self.kind(n[2])
        if k == "member":
            base = self.kind(n[1])
            if base in VEC and n[2] in ("x", "y", "z", "w", "r", "g", "b", "a"):
                return "float"
            return "num"
        if k == "index":
            return "num"
        if k == "ternary":
            a, b = self.kind(n[2]), self.kind(n[3])
            return "float" if "float" in (a, b) else a
        if k == "call":
            fn = n[1]
            if fn[0] == "name":
                nm = fn[1]
                if nm in TYPE_CTORS:
                    return {"Vector2": "vec2", "Vector3": "vec3", "Quaternion": "quat", "Color": "color"}.get(nm, "other")
                if nm in INT_FUNCS or nm in ("int", "toInt"):
                    return "int"
                if nm in FLOAT_FUNCS or nm in ("float", "toFloat"):
                    return "float"
                if nm in self.funcret:
                    return self.funcret[nm]
            if fn[0] == "member":
                m = fn[2]
                if m in ("length", "dot", "distance_to"):
                    return "float"
                if m in ("normalized", "lerp", "add", "sub", "cross"):
                    return self.kind(fn[1])
            return "num"
        if k == "bin":
            op = n[1]
            if op in ("and", "or", "==", "!=", "<", ">", "<=", ">=", "in"):
                return "bool"
            la, ra = self.kind(n[2]), self.kind(n[3])
            if la in VEC:
                return la
            if op in ("&", "|", "^", "<<", ">>"):
                return "int"
            if op == "/":
                return "float"
            if la == "int" and ra == "int":
                return "int"
            return "float" if "float" in (la, ra) else "num"
        return "other"


# ── emitter ─────────────────────────────────────────────────────────────────────
class Emit:
    def __init__(self, decls):
        self.decls = decls
        self.funcret = {}
        for d in decls:
            if d[0] == "func":
                self.funcret[d[2]] = None
        self.T = Types(self._retmap())

    def _retmap(self):
        m = {}
        for d in self.decls:
            if d[0] == "func":
                m[d[2]] = self.T_norm(d[4]) if hasattr(self, "T") else None
        return {d[2]: {"int": "int", "float": "float", "bool": "bool", "Vector2": "vec2",
                        "Vector3": "vec3", "Quaternion": "quat", "Color": "color",
                        "Array": "array", "Dictionary": "dict", "String": "str"}.get(d[4], "num")
                for d in self.decls if d[0] == "func"}

    def module(self):
        out = ['import * as gd from "./runtime.mjs";', ""]
        for d in self.decls:
            if d[0] == "const":
                out.append(f"export const {d[1]} = {self.expr(d[2])};")
            elif d[0] == "func":
                out.append(self.func(d))
                out.append("")
        return "\n".join(out) + "\n"

    def func(self, d):
        _, is_static, name, params, ret, body = d
        self.T.env = {}
        for p in params:
            self.T.env[p[0]] = self.T.norm(p[1])
        ps = ", ".join(p[0] + (f" = {self.expr(p[2])}" if p[2] else "") for p in params)
        head = f"export function {name}({ps}) {{"
        lines = self.block(body, 1)
        return head + "\n" + lines + "\n}"

    def block(self, stmts, ind):
        pad = "\t" * ind
        out = []
        for s in stmts:
            for ln in self.stmt(s, ind):
                out.append(pad + ln)
        return "\n".join(out)

    def stmt(self, s, ind):
        k = s[0]
        if k == "var":
            _, name, typ, val = s
            self.T.env[name] = self.T.norm(typ) if typ else (self.T.kind(val) if val else "num")
            return [f"let {name}" + (f" = {self.expr(val)};" if val is not None else ";")]
        if k == "return":
            return ["return" + (f" {self.expr(s[1])};" if s[1] is not None else ";")]
        if k == "expr":
            return [self.expr(s[1]) + ";"]
        if k == "assign":
            _, op, lhs, rhs = s
            L = self.expr(lhs)
            if op == "=":
                return [f"{L} = {self.expr(rhs)};"]
            base = op[0]
            return [f"{L} = {self.binop(base, lhs, rhs)};"]
        if k in ("pass",):
            return []
        if k in ("break", "continue"):
            return [k + ";"]
        if k == "if":
            _, cond, body, elifs, els = s
            lines = [f"if ({self.expr(cond)}) {{", self.block(body, ind + 1), "}"]
            res = self._joinblock(lines)
            for c, b in elifs:
                res += self._joinblock([f"else if ({self.expr(c)}) {{", self.block(b, ind + 1), "}"], lead=" ")
            if els is not None:
                res += self._joinblock(["else {", self.block(els, ind + 1), "}"], lead=" ")
            return res.split("\n")
        if k == "for":
            _, name, it, body = s
            self.T.env[name] = "num"
            if it[0] == "call" and it[1][0] == "name" and it[1][1] == "range":
                args = it[2]
                if len(args) == 1:
                    init, cond, step = "0", f"{self.expr(args[0])}", "++"
                    head = f"for (let {name} = 0; {name} < {self.expr(args[0])}; {name}++) {{"
                elif len(args) == 2:
                    head = f"for (let {name} = {self.expr(args[0])}; {name} < {self.expr(args[1])}; {name}++) {{"
                else:
                    head = f"for (let {name} = {self.expr(args[0])}; {name} < {self.expr(args[1])}; {name} += {self.expr(args[2])}) {{"
            else:
                head = f"for (const {name} of {self.expr(it)}) {{"
            return self._joinblock([head, self.block(body, ind + 1), "}"]).split("\n")
        if k == "while":
            _, c, b = s
            return self._joinblock([f"while ({self.expr(c)}) {{", self.block(b, ind + 1), "}"]).split("\n")
        if k == "match":
            _, subj, cases = s
            out = [f"switch ({self.expr(subj)}) {{"]
            for pats, body in cases:
                if len(pats) == 1 and pats[0] == ("name", "_"):
                    out.append("default: {")
                else:
                    for p in pats:
                        out.append(f"case {self.expr(p)}:")
                    out[-1] = out[-1]
                    out.append("{")
                out.append(self.block(body, ind + 2))
                out.append("break; }")
            out.append("}")
            return "\n".join(out).split("\n")
        raise SyntaxError(f"emit stmt {k}")

    def _joinblock(self, parts, lead=""):
        # parts = [head, body(maybe empty), tail]
        res = (lead + parts[0])
        if parts[1].strip():
            res += "\n" + parts[1]
        res += "\n" + parts[2] if len(parts) > 2 else ""
        return res

    def expr(self, n):
        k = n[0]
        if k == "lit":
            if n[1] == "hex":
                return n[2]
            if n[1] == "str":
                return '"' + n[2][1:-1].replace('"', '\\"') + '"' if n[2][0] == "'" else n[2]
            return n[2].replace("_", "")
        if k == "kw":
            return {"true": "true", "false": "false", "null": "null", "self": "this"}[n[1]]
        if k == "name":
            if n[1] in ("PI", "TAU"):
                return "gd." + n[1]
            return n[1]
        if k == "paren":
            return f"({self.expr(n[1])})"
        if k == "unary":
            op = {"not": "!", "-": "-", "+": "+", "~": "~"}[n[1]]
            return f"{op}{self.expr(n[2])}"
        if k == "member":
            base = n[1]
            if base[0] == "name" and base[1] in TYPE_CTORS:
                return f"gd.{base[1]}.{n[2]}"
            return f"{self.expr(base)}.{n[2]}"
        if k == "index":
            return f"{self.expr(n[1])}[{self.expr(n[2])}]"
        if k == "ternary":
            return f"({self.expr(n[1])} ? {self.expr(n[2])} : {self.expr(n[3])})"
        if k == "array":
            return "[" + ", ".join(self.expr(e) for e in n[1]) + "]"
        if k == "dict":
            return "{" + ", ".join(f"[{self.expr(kk)}]: {self.expr(vv)}" for kk, vv in n[1]) + "}"
        if k == "call":
            return self.call(n)
        if k == "bin":
            return self.binop(n[1], n[2], n[3])
        raise SyntaxError(f"emit expr {k}")

    def call(self, n):
        fn, args = n[1], n[2]
        A = ", ".join(self.expr(a) for a in args)
        if fn[0] == "name":
            nm = fn[1]
            if nm in TYPE_CTORS:
                return f"new gd.{nm}({A})"
            if nm in GLOBAL_FUNCS:
                return f"gd.{nm}({A})"
            if nm == "int":
                return f"gd.toInt({A})"
            if nm == "float":
                return f"gd.toFloat({A})"
            if nm == "Vector2" or nm == "Vector3":
                return f"new gd.{nm}({A})"
        if fn[0] == "member" and fn[2] == "new":
            # Foo.new(...) -> new Foo(...)
            return f"new {self.expr(fn[1])}({A})"
        return f"{self.expr(fn)}({A})"

    def binop(self, op, ln, rn):
        L, R = self.expr(ln), self.expr(rn)
        lk = self.T.kind(ln)
        if op == "and":
            return f"({L} && {R})"
        if op == "or":
            return f"({L} || {R})"
        if op == "in":
            return f"{R}.includes({L})"
        if op == "==":
            return f"({L} === {R})"
        if op == "!=":
            return f"({L} !== {R})"
        if op in ("<", ">", "<=", ">="):
            return f"({L} {op} {R})"
        # vector operators
        if lk in VEC:
            if op == "+":
                return f"{L}.add({R})"
            if op == "-":
                return f"{L}.sub({R})"
            if op == "*":
                return f"{L}.mul({R})"
            if op == "/":
                return f"{L}.div({R})"
        # integer 32-bit ops (keep hash math bit-identical)
        if op in ("&", "|", "^", "<<"):
            return f"(({L} {op} {R}) >>> 0)"
        if op == ">>":
            return f"({L} >>> {R})"
        if op == "*" and lk == "int" and self.T.kind(rn) == "int":
            return f"gd.imul32({L}, {R})"
        return f"({L} {op} {R})"


def transpile(src):
    toks = lex(src)
    decls = Parser(toks).parse_module()
    decls = [d for d in decls if d[0] != "skip"]
    return Emit(decls).module()


if __name__ == "__main__":
    with open(sys.argv[1], encoding="utf-8") as f:
        sys.stdout.write(transpile(f.read()))
