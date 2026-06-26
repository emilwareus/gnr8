from __future__ import annotations

import enum
from dataclasses import dataclass
from typing import Any, Dict, List, Literal, Optional, Union

class Availability(str, enum.Enum):
    IN_STOCK = "in_stock"
    OUT_OF_STOCK = "out_of_stock"

@dataclass
class OrderConfirmation:
    availability: Availability
    lines: List[Price]
    message: Optional[str]
    order_id: int
    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "OrderConfirmation":
        return cls(
            availability=_data["availability"],
            lines=[Price.from_dict(_item) for _item in _data["lines"]],
            message=_data["message"],
            order_id=_data["order_id"],
        )

@dataclass
class OrderInput:
    book_id: int
    price: Price
    coupon: Optional[str] = None
    discount: Optional[Union[int, float]] = None
    note: Optional[str] = None
    quantity: Optional[int] = None
    tags: Optional[List[str]] = None
    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "OrderInput":
        return cls(
            book_id=_data["book_id"],
            coupon=(_data["coupon"]) if "coupon" in _data and _data["coupon"] is not None else None,
            discount=(_data["discount"]) if "discount" in _data and _data["discount"] is not None else None,
            note=(_data["note"]) if "note" in _data and _data["note"] is not None else None,
            price=Price.from_dict(_data["price"]),
            quantity=(_data["quantity"]) if "quantity" in _data and _data["quantity"] is not None else None,
            tags=(_data["tags"]) if "tags" in _data and _data["tags"] is not None else None,
        )

@dataclass
class Price:
    amount: float
    currency: Literal["eur", "usd"]
    @classmethod
    def from_dict(cls, _data: Dict[str, Any]) -> "Price":
        return cls(
            amount=_data["amount"],
            currency=_data["currency"],
        )
