"""sparkid - Fast, time-sortable, 22-char Base58 unique ID generator."""

from sparkid._generator import IdGenerator, generate_id

__all__ = ["generate_id", "IdGenerator"]
