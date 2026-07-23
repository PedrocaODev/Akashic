# Slice 3 generation disposition

No Rust generation newtype is added: this contract requires stable identified
generations, not a distinct Rust type. Generation identity remains hash-based
and typed encoding plus the Slice 3 tests preserve that contract.
