"""sparkid - Fast, time-sortable, 21-char Base58 unique ID generator."""

from sparkid._generator import IdGenerator, generate_id, extract_timestamp

__all__ = ["generate_id", "extract_timestamp", "IdGenerator"]
