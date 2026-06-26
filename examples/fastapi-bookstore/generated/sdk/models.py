from __future__ import annotations

import enum
from dataclasses import dataclass
from typing import Any, Dict, List, Literal, Optional, Union

@dataclass
class Author:
    bio: Optional[str]
    name: str
    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "Author":
        return cls(
            bio=_data["bio"],
            name=_data["name"],
        )

@dataclass
class Book:
    author: Author
    format: BookFormat
    id: int
    title: str
    rating: Optional[Union[int, float]] = None
    tags: Optional[List[str]] = None
    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "Book":
        return cls(
            author=Author.from_dict(_data["author"]),
            format=_data["format"],
            id=_data["id"],
            rating=(_data["rating"]) if "rating" in _data and _data["rating"] is not None else None,
            tags=(_data["tags"]) if "tags" in _data and _data["tags"] is not None else None,
            title=_data["title"],
        )

@dataclass
class BookFilters:
    genre: str
    published: Optional[int]
    in_stock: Optional[bool] = None
    sort: Optional[Literal["asc", "desc"]] = None
    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "BookFilters":
        return cls(
            genre=_data["genre"],
            in_stock=(_data["in_stock"]) if "in_stock" in _data and _data["in_stock"] is not None else None,
            published=_data["published"],
            sort=(_data["sort"]) if "sort" in _data and _data["sort"] is not None else None,
        )

class BookFormat(str, enum.Enum):
    HARDCOVER = "hardcover"
    PAPERBACK = "paperback"

BookOrError = "Union[Book, OutOfStock]"

@dataclass
class CreatedMessage:
    id: int
    message: str
    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "CreatedMessage":
        return cls(
            id=_data["id"],
            message=_data["message"],
        )

@dataclass
class ListBooksResponse:
    books: List[Book]
    next_cursor: Optional[str]
    total: int
    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "ListBooksResponse":
        return cls(
            books=[Book.from_dict(_item) for _item in _data["books"]],
            next_cursor=_data["next_cursor"],
            total=_data["total"],
        )

@dataclass
class OutOfStock:
    reason: str
    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "OutOfStock":
        return cls(
            reason=_data["reason"],
        )
