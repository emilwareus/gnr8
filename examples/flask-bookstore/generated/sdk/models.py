from __future__ import annotations

import enum
from typing import TYPE_CHECKING, Any, Dict, List, Literal, Optional, Union

from pydantic import BaseModel, ConfigDict, Field

class Availability(str, enum.Enum):
    IN_STOCK = "in_stock"
    OUT_OF_STOCK = "out_of_stock"

class OrderConfirmation(BaseModel):
    model_config = ConfigDict(populate_by_name=True, extra="ignore")
    availability: Availability
    lines: List[Price]
    message: Optional[str]
    order_id: int

    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "OrderConfirmation":
        return cls.model_validate(_data)

    def to_dict(self) -> Dict[str, Any]:
        return self.model_dump(mode="json", by_alias=True, exclude_none=True)

class OrderInput(BaseModel):
    model_config = ConfigDict(populate_by_name=True, extra="ignore")
    book_id: int
    coupon: Optional[str] = Field(default=None)
    discount: Optional[Union[int, float]] = Field(default=None)
    note: Optional[str] = Field(default=None)
    price: Price
    quantity: Optional[int] = Field(default=None)
    tags: Optional[List[str]] = Field(default=None)

    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "OrderInput":
        return cls.model_validate(_data)

    def to_dict(self) -> Dict[str, Any]:
        return self.model_dump(mode="json", by_alias=True, exclude_none=True)

class Price(BaseModel):
    model_config = ConfigDict(populate_by_name=True, extra="ignore")
    amount: float
    currency: Literal["eur", "usd"]

    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "Price":
        return cls.model_validate(_data)

    def to_dict(self) -> Dict[str, Any]:
        return self.model_dump(mode="json", by_alias=True, exclude_none=True)
