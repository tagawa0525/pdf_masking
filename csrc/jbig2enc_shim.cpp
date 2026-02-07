// Thin C shim around jbig2enc's C++ jbig2_encode_generic function.
//
// jbig2enc exposes jbig2_encode_generic with C++ linkage (name-mangled).
// This shim re-exports it with extern "C" linkage so Rust can call it
// through a stable C ABI without relying on fragile mangled symbol names.

#include <leptonica/allheaders.h>

// jbig2_encode_generic is declared in jbig2enc's source but not in
// its installed public headers.  We provide the declaration here.
//
// Signature (from jbig2enc source):
//   unsigned char *jbig2_encode_generic(
//       Pix *pix,
//       bool duplicate_line_removal,
//       int tpl_x, int tpl_y,
//       bool use_refinement,
//       int *length);
//
// Returns a malloc'd buffer that the caller must free().
// On failure returns NULL and *length is undefined.
extern unsigned char *jbig2_encode_generic(
    PIX *pix,
    bool duplicate_line_removal,
    int tpl_x,
    int tpl_y,
    bool use_refinement,
    int *length);

extern "C" {

/// C-ABI wrapper around jbig2_encode_generic.
///
/// Converts int flags (0/1) to C++ bool for ABI safety.
/// Returns NULL on failure.
unsigned char *jbig2enc_encode_generic_c(
    PIX *pix,
    int duplicate_line_removal,
    int tpl_x,
    int tpl_y,
    int use_refinement,
    int *length)
{
    return jbig2_encode_generic(
        pix,
        duplicate_line_removal != 0,
        tpl_x,
        tpl_y,
        use_refinement != 0,
        length);
}

}  // extern "C"
