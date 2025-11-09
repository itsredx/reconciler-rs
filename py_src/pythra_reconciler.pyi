# ==================== py_src/pythra_reconciler.pyi ====================
"""
Type stubs for the PyThra Reconciler Rust extension
"""

from typing import Any, Dict, List, Optional, Tuple, Union, Callable, Literal
from dataclasses import dataclass, field

PatchAction = Literal["INSERT", "REMOVE", "UPDATE", "MOVE", "REPLACE"]

@dataclass
class Key:
    value: Any
    
    def __init__(self, value: Any) -> None: ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

@dataclass 
class Patch:
    action: PatchAction
    html_id: str
    data: Dict[str, Any]

@dataclass
class ReconciliationResult:
    patches: List[Patch] = field(default_factory=list)
    new_rendered_map: Dict[Union[Key, str], Any] = field(default_factory=dict)
    active_css_details: Dict[str, Tuple[Callable, Any]] = field(default_factory=dict)
    registered_callbacks: Dict[str, Callable] = field(default_factory=dict)
    js_initializers: List[Dict] = field(default_factory=list)

class Reconciler:
    def __init__(self) -> None: ...
    
    def clear_context(self, context_key: str) -> None: ...
    
    def clear_all_contexts(self) -> None: ...
    
    def reconcile(
        self,
        previous_map: Dict[Union[Key, str], Any],
        new_widget_root: Any,
        parent_html_id: str,
        old_root_key: Optional[Union[Key, str]] = None,
        is_partial_reconciliation: bool = False,
    ) -> ReconciliationResult: ...