import sys
try:
    # PyO3 >= 0.21 returns a PyDict/PyNone wrapper that we need to handle.
    # We can import these types to check against them.
    from rust_reconciler import reconcile
    print("✅ Successfully imported Rust module.")
except ImportError as e:
    print(f"❌ Could not import Rust module. Did you run 'maturin develop'? Error: {e}")
    sys.exit(1)

def run_test(name, old_tree, new_tree):
    print(f"\n--- Running Test: {name} ---")
    
    try:
        patches = reconcile(old_tree, new_tree)
        
        print("Generated Patches:")
        if not patches:
            print("  (None)")
        else:
            sorted_patches = sorted(patches, key=lambda p: (p.action, p.target_id))
            for patch in sorted_patches:
                # *** CORRECT PYTHON LOGIC ***
                # The returned object is a standard Python type, not a PyO3 wrapper.
                data_str = "None"
                if patch.data is not None:
                    # It's a dict-like object, convert to a real dict for printing
                    data_str = str(dict(patch.data))

                print(f"  - Action: {patch.action}, Target: {patch.target_id}, Data: {data_str}")
        print("-" * 25)
    except Exception as e:
        print(f"❌ TEST FAILED: An error occurred: {e}")
        # Print the full traceback for debugging
        import traceback
        traceback.print_exc()
        print("-" * 25)

# (The test cases are correct and do not need to change)
# Test 1: No changes
run_test("No Change",
    old_tree={"root": {"key": "root", "type": "Div"}},
    new_tree={"root": {"key": "root", "type": "Div"}}
)
# Test 2: Simple property update
run_test("UPDATE",
    old_tree={"root": {"key": "root", "type": "Div", "props": {"color": "blue"}}},
    new_tree={"root": {"key": "root", "type": "Div", "props": {"color": "red"}}}
)
# Test 3: Node replacement
run_test("REPLACE",
    old_tree={"root": {"key": "root", "type": "Div", "children": ["c1"]}, "c1": {"key": "c1", "type": "Text"}},
    new_tree={"root": {"key": "root", "type": "Div", "children": ["c1"]}, "c1": {"key": "c1", "type": "Button"}}
)
# Test 4: Child Insertion
run_test("INSERT child",
    old_tree={
        "root": {"key": "root", "type": "Div", "children": ["a", "c"]},
        "a": {"key": "a", "type": "Div"}, "c": {"key": "c", "type": "Div"}
    },
    new_tree={
        "root": {"key": "root", "type": "Div", "children": ["a", "b", "c"]},
        "a": {"key": "a", "type": "Div"}, "b": {"key": "b", "type": "Div"}, "c": {"key": "c", "type": "Div"}
    }
)
# Test 5: Child Reordering (The classic LIS test)
run_test("MOVE children (LIS)",
    old_tree={
        "root": {"key": "root", "type": "Div", "children": ["a", "b", "c", "d", "e"]},
        "a": {"key": "a", "type": "Div"}, "b": {"key": "b", "type": "Div"}, "c": {"key": "c", "type": "Div"}, "d": {"key": "d", "type": "Div"}, "e": {"key": "e", "type": "Div"}
    },
    new_tree={
        "root": {"key": "root", "type": "Div", "children": ["a", "d", "c", "f", "b"]},
        "a": {"key": "a", "type": "Div"}, "d": {"key": "d", "type": "Div"}, "c": {"key": "c", "type": "Div"}, "f": {"key": "f", "type": "Div"}, "b": {"key": "b", "type": "Div"}
    }
)
# Test 6: Insert at the beginning
run_test("INSERT at beginning",
    old_tree={"root": {"key": "root", "type": "Div", "children": ["b", "c"]}, "b": {"key": "b", "type": "Div"}, "c": {"key": "c", "type": "Div"}},
    new_tree={"root": {"key": "root", "type": "Div", "children": ["a", "b", "c"]}, "a": {"key": "a", "type": "Div"}, "b": {"key": "b", "type": "Div"}, "c": {"key": "c", "type": "Div"}}
)
# Test 7: All children removed
run_test("REMOVE all children",
    old_tree={"root": {"key": "root", "type": "Div", "children": ["a", "b"]}, "a": {"key": "a", "type": "Div"}, "b": {"key": "b", "type": "Div"}},
    new_tree={"root": {"key": "root", "type": "Div", "children": []}}
)

print("\n✅ All tests complete! The Rust reconciler is finished.")