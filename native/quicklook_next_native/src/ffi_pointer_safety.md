# Safety

Every accessed input pointer must be readable for its paired byte length. Every accessed
byte-output pointer must contain initialized bytes and be exclusively writable for its paired
capacity. Each accessed range must belong to one live allocation. Scalar output pointers must
also have the alignment of their
pointed-to type. Output ranges must not overlap any input range or other output range.
Inputs must remain immutable while borrowed. All accessed buffers and callbacks must remain
valid for the duration of the call; callbacks must not unwind across the C ABI. Invalid pointers,
including null pointers, are allowed only on documented size-query or rejection paths that
return before accessing them; a null/length check cannot validate any other pointer.
