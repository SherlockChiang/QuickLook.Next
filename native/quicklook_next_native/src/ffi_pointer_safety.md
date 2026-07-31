# Safety

Every non-null input pointer must be readable for its paired byte length, and every non-null
output pointer must be writable for its paired capacity. All pointed-to buffers and callbacks must
remain valid for the duration of the call.
