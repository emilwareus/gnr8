from __future__ import annotations

import enum
from typing import TYPE_CHECKING, Any, Dict, List, Literal, Optional, Union

from pydantic import BaseModel, ConfigDict, Field

class Author(BaseModel):
    model_config = ConfigDict(populate_by_name=True, extra="ignore")
    bio: Optional[str]
    name: str

    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "Author":
        return cls.model_validate(_data)

    def to_dict(self) -> Dict[str, Any]:
        return self.model_dump(mode="json", by_alias=True, exclude_none=True)

class Book(BaseModel):
    model_config = ConfigDict(populate_by_name=True, extra="ignore")
    author: Author
    format: BookFormat
    id: int
    rating: Optional[Union[int, float]] = Field(default=None)
    tags: Optional[List[str]] = Field(default=None)
    title: str

    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "Book":
        return cls.model_validate(_data)

    def to_dict(self) -> Dict[str, Any]:
        return self.model_dump(mode="json", by_alias=True, exclude_none=True)

class BookFilters(BaseModel):
    model_config = ConfigDict(populate_by_name=True, extra="ignore")
    genre: str
    in_stock: Optional[bool] = Field(default=None)
    published: Optional[int]
    sort: Optional[Literal["asc", "desc"]] = Field(default=None)

    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "BookFilters":
        return cls.model_validate(_data)

    def to_dict(self) -> Dict[str, Any]:
        return self.model_dump(mode="json", by_alias=True, exclude_none=True)

class BookFormat(str, enum.Enum):
    HARDCOVER = "hardcover"
    PAPERBACK = "paperback"

BookOrError = "Union[Book, OutOfStock]"

class CreatedMessage(BaseModel):
    model_config = ConfigDict(populate_by_name=True, extra="ignore")
    id: int
    message: str

    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "CreatedMessage":
        return cls.model_validate(_data)

    def to_dict(self) -> Dict[str, Any]:
        return self.model_dump(mode="json", by_alias=True, exclude_none=True)

class ListBooksResponse(BaseModel):
    model_config = ConfigDict(populate_by_name=True, extra="ignore")
    books: List[Book]
    next_cursor: Optional[str]
    total: int

    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "ListBooksResponse":
        return cls.model_validate(_data)

    def to_dict(self) -> Dict[str, Any]:
        return self.model_dump(mode="json", by_alias=True, exclude_none=True)

class OutOfStock(BaseModel):
    model_config = ConfigDict(populate_by_name=True, extra="ignore")
    reason: str

    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "OutOfStock":
        return cls.model_validate(_data)

    def to_dict(self) -> Dict[str, Any]:
        return self.model_dump(mode="json", by_alias=True, exclude_none=True)
