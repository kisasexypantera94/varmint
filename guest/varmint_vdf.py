"""Small lossless Valve Data Format reader/editor used by Vet.

It parses the structure instead of treating VDF as regex-editable text, while
replacing only the requested value (or inserting the missing subtree).
"""

from dataclasses import dataclass
from pathlib import Path
import struct


class VdfError(ValueError):
    pass


APPINFO_V29 = 0x07564429


def read_appinfo(path):
    data = Path(path).read_bytes()
    if len(data) < 20 or struct.unpack_from("<I", data)[0] != APPINFO_V29:
        raise VdfError("unsupported Steam appinfo format")
    table_offset = struct.unpack_from("<Q", data, 8)[0]
    if table_offset < 20 or table_offset + 4 > len(data):
        raise VdfError("invalid Steam appinfo key table")

    key_count = struct.unpack_from("<I", data, table_offset)[0]
    position = table_offset + 4
    keys = []
    for _ in range(key_count):
        end = data.find(b"\0", position)
        if end < 0:
            raise VdfError("unterminated Steam appinfo key")
        keys.append(data[position:end].decode("utf-8", errors="replace"))
        position = end + 1

    def parse_object(offset, depth=0):
        if depth > 64:
            raise VdfError("Steam appinfo nesting is too deep")
        result = {}
        while offset < table_offset:
            kind = data[offset]
            offset += 1
            if kind == 8:
                return result, offset
            if offset + 4 > table_offset:
                break
            key_index = struct.unpack_from("<I", data, offset)[0]
            offset += 4
            if key_index >= len(keys):
                raise VdfError("invalid Steam appinfo key index")
            key = keys[key_index]
            if kind == 0:
                value, offset = parse_object(offset, depth + 1)
            elif kind == 1:
                end = data.find(b"\0", offset, table_offset)
                if end < 0:
                    raise VdfError("unterminated Steam appinfo string")
                value = data[offset:end].decode("utf-8", errors="replace")
                offset = end + 1
            elif kind == 2:
                value = struct.unpack_from("<I", data, offset)[0]
                offset += 4
            elif kind == 3:
                value = struct.unpack_from("<f", data, offset)[0]
                offset += 4
            elif kind == 7:
                value = struct.unpack_from("<Q", data, offset)[0]
                offset += 8
            elif kind == 10:
                value = struct.unpack_from("<q", data, offset)[0]
                offset += 8
            else:
                raise VdfError(f"unsupported Steam appinfo value type {kind}")
            result[key] = value
        raise VdfError("unterminated Steam appinfo object")

    applications = []
    position = 16
    while position + 4 <= table_offset:
        appid = struct.unpack_from("<I", data, position)[0]
        if appid == 0:
            break
        if position + 68 > table_offset:
            raise VdfError("truncated Steam appinfo record")
        value, next_position = parse_object(position + 68)
        if next_position <= position:
            raise VdfError("invalid Steam appinfo record size")
        applications.append((appid, value.get("appinfo", {})))
        position = next_position
    return applications


@dataclass
class Token:
    value: str
    start: int
    end: int
    kind: str


@dataclass
class Node:
    key: str
    key_token: Token
    value: str | None = None
    value_token: Token | None = None
    children: list | None = None
    close_start: int | None = None

    def child(self, key):
        if self.children is None:
            return None
        wanted = key.casefold()
        return next(
            (node for node in self.children if node.key.casefold() == wanted), None
        )


def _quoted(text, offset):
    out = []
    i = offset + 1
    while i < len(text):
        char = text[i]
        if char == '"':
            return Token("".join(out), offset, i + 1, "string"), i + 1
        if char == "\\":
            i += 1
            if i >= len(text):
                raise VdfError("unterminated VDF escape")
            escaped = text[i]
            out.append({"n": "\n", "t": "\t", "r": "\r"}.get(escaped, escaped))
        else:
            out.append(char)
        i += 1
    raise VdfError("unterminated VDF string")


def tokenize(text):
    tokens = []
    i = 0
    while i < len(text):
        if text[i].isspace():
            i += 1
            continue
        if text.startswith("//", i):
            newline = text.find("\n", i + 2)
            i = len(text) if newline < 0 else newline + 1
            continue
        if text[i] == '"':
            token, i = _quoted(text, i)
            tokens.append(token)
            continue
        if text[i] in "{}":
            tokens.append(Token(text[i], i, i + 1, text[i]))
            i += 1
            continue
        start = i
        while i < len(text) and not text[i].isspace() and text[i] not in '{}"':
            i += 1
        tokens.append(Token(text[start:i], start, i, "string"))
    return tokens


class Document:
    def __init__(self, text):
        self.text = text
        self.tokens = tokenize(text)
        self.index = 0
        self.nodes, closing = self._entries(False)
        if closing is not None or self.index != len(self.tokens):
            raise VdfError("unexpected closing brace")

    def _entries(self, nested):
        nodes = []
        while self.index < len(self.tokens):
            token = self.tokens[self.index]
            if token.kind == "}":
                if not nested:
                    raise VdfError("unexpected closing brace")
                self.index += 1
                return nodes, token.start
            if token.kind != "string":
                raise VdfError(f"expected VDF key at byte {token.start}")
            key = token
            self.index += 1
            if self.index >= len(self.tokens):
                raise VdfError(f"missing value for {key.value!r}")
            value = self.tokens[self.index]
            self.index += 1
            if value.kind == "{":
                children, close_start = self._entries(True)
                nodes.append(
                    Node(key.value, key, children=children, close_start=close_start)
                )
            elif value.kind == "string":
                nodes.append(Node(key.value, key, value=value.value, value_token=value))
            else:
                raise VdfError(f"expected value for {key.value!r}")
        if nested:
            raise VdfError("unterminated VDF object")
        return nodes, None

    def find(self, path):
        nodes = self.nodes
        node = None
        for part in path:
            wanted = part.casefold()
            node = next(
                (entry for entry in nodes if entry.key.casefold() == wanted), None
            )
            if node is None:
                return None
            nodes = node.children or []
        return node

    def value(self, path, default=None):
        node = self.find(path)
        return node.value if node is not None and node.children is None else default

    def set_value(self, path, value):
        if not path:
            raise VdfError("empty VDF path")
        encoded = quote(value)
        existing = self.find(path)
        if existing is not None:
            if existing.value_token is None:
                raise VdfError("target VDF path is an object")
            token = existing.value_token
            return self.text[: token.start] + encoded + self.text[token.end :]

        nodes = self.nodes
        parent = None
        matched = 0
        for part in path[:-1]:
            wanted = part.casefold()
            found = next(
                (entry for entry in nodes if entry.key.casefold() == wanted), None
            )
            if found is None:
                break
            if found.children is None:
                raise VdfError(f"VDF parent {part!r} is a value")
            parent = found
            nodes = found.children
            matched += 1

        insertion = len(self.text) if parent is None else parent.close_start
        if insertion is None:
            raise VdfError("VDF parent has no closing brace")
        missing = path[matched:]
        depth = matched + (1 if parent is not None else 0)
        lines = []
        for index, part in enumerate(missing[:-1]):
            indent = "\t" * (depth + index)
            lines.extend((f"{indent}{quote(part)}", f"{indent}{{"))
        leaf_indent = "\t" * (depth + len(missing) - 1)
        lines.append(f"{leaf_indent}{quote(missing[-1])}\t\t{encoded}")
        for index in reversed(range(len(missing) - 1)):
            lines.append("\t" * (depth + index) + "}")
        snippet = "\n".join(lines) + "\n"
        if insertion > 0 and self.text[insertion - 1] != "\n":
            snippet = "\n" + snippet
        return self.text[:insertion] + snippet + self.text[insertion:]

    def delete_value(self, path):
        existing = self.find(path)
        if existing is None:
            return self.text
        if existing.value_token is None:
            raise VdfError("target VDF path is an object")

        start = existing.key_token.start
        end = existing.value_token.end
        while start > 0 and self.text[start - 1] in " \t":
            start -= 1
        if start > 0 and self.text[start - 1] == "\n":
            start -= 1
        while end < len(self.text) and self.text[end] in " \t":
            end += 1
        if end < len(self.text) and self.text[end] == "\n":
            end += 1
        return self.text[:start] + self.text[end:]

    def delete_node(self, path):
        existing = self.find(path)
        if existing is None:
            return self.text
        if existing.children is None or existing.close_start is None:
            raise VdfError("target VDF path is a value")

        start = existing.key_token.start
        end = existing.close_start + 1
        while start > 0 and self.text[start - 1] in " \t":
            start -= 1
        if start > 0 and self.text[start - 1] == "\n":
            start -= 1
        while end < len(self.text) and self.text[end] in " \t":
            end += 1
        if end < len(self.text) and self.text[end] == "\n":
            end += 1
        return self.text[:start] + self.text[end:]


def quote(value):
    if not isinstance(value, str):
        raise TypeError("VDF values must be strings")
    escaped = value.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")
    return f'"{escaped}"'
