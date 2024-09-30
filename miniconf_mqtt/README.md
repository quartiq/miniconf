# `miniconf` MQTT Client

This package contains a MQTT client exposing a [`miniconf`](https://crates.io/crates/miniconf) interface via MQTT using [`minimq`](https://crates.io/crates/minimq).

## Command types

| Command | Node | Response Topic | Payload |
| --- | --- | --- | --- |
| Get | Leaf | set | empty |
| List | Internal | set | empty |
| Dump | (any) | not set | empty |
| Set | Leaf | | some |
| (Error) | Internal |  | some |

## Notes

* `List` list paths that would result in `miniconf::Traversal::Absent` on `Get` or `Set`.
