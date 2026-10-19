/*
 * font_shim.c - the only C translation unit in the Switch build.
 *
 * stb_truetype keeps its state in large, pointer-heavy structs
 * (stbtt_fontinfo / stbtt__buf / stbtt_vertex) that are an implementation
 * detail, not an ABI.  Mirroring them in Rust would be a silent-corruption
 * trap, so they stay on this side of the FFI and Rust only ever sees opaque
 * handles and plain integers:
 *
 *   void* mhnx_font_create(const unsigned char* data, int len, float pixel_height);
 *   int   mhnx_font_ascent(void* f);
 *   int   mhnx_font_descent(void* f);
 *   int   mhnx_font_line_gap(void* f);
 *   int   mhnx_font_cff(void* f);
 *   int   mhnx_font_glyph(void* f, int codepoint, unsigned char* out, int out_cap,
 *                         int* w, int* h, int* xoff, int* yoff, int* advance);
 *   void  mhnx_font_destroy(void* f);
 *
 * mhnx_font_glyph() return values:
 *   -2  codepoint is not mapped by the font
 *   -1  invalid / destroyed handle
 *    0  rasterised: if w and h are positive, out[0 .. w*h) now holds an
 *       8-bit coverage bitmap with row stride == w.  w or h may be 0 (the
 *       space character): nothing to draw, only the advance matters.
 *   >0  "metrics only": the bitmap needs this many bytes (w*h) and nothing
 *       was written, because out == NULL or out_cap < w*h.  w/h/xoff/yoff/
 *       advance are valid.  Call again with out_cap >= the returned value.
 *
 * Coordinates follow stb_truetype: xoff/yoff are the offset of the bitmap's
 * top-left corner from the pen position / baseline, y growing downwards.
 *
 * The font data is NOT copied; it must stay mapped while the handle is alive.
 * On the Switch that is pl:u shared memory, which stays mapped until plExit().
 *
 * Handle storage is a fixed static pool, so this file never allocates on
 * behalf of the caller (stb_truetype itself still uses malloc/free internally
 * for glyph outlines; that is newlib's heap, i.e. the same one Rust uses).
 */

#include <stddef.h>
#include <stdlib.h>
#include <string.h>

/* ------------------------------------------------------------------------- */
/* stb_truetype configuration - must precede the implementation include      */
/* ------------------------------------------------------------------------- */

/* Never abort the console from inside the rasteriser: stb only uses it for
   "this should be impossible" checks. */
#define STBTT_assert(x) ((void)0)

/* floor()/ceil()/fabs() sit on the rasteriser's hot path.  Overriding them
   keeps those three out of libm (the macros are only defined by stb when they
   are still undefined, so this has to come before the include). */
static int mhnx_ifloor(double x)
{
    int i = (int) x;
    return ((double) i > x) ? i - 1 : i;
}

static int mhnx_iceil(double x)
{
    int i = (int) x;
    return ((double) i < x) ? i + 1 : i;
}

static double mhnx_fabs(double x)
{
    return x < 0.0 ? -x : x;
}

#define STBTT_ifloor(x) mhnx_ifloor((double) (x))
#define STBTT_iceil(x)  mhnx_iceil((double) (x))
#define STBTT_fabs(x)   mhnx_fabs((double) (x))

#define STB_TRUETYPE_IMPLEMENTATION
#include "stb_truetype.h"

/* ------------------------------------------------------------------------- */
/* handle pool                                                               */
/* ------------------------------------------------------------------------- */

#define MHNX_MAX_FONTS 4

struct mhnx_font {
    stbtt_fontinfo info;
    float scale;        /* font units -> pixels, scaled for pixel_height */
    int   ascent;       /* pixels above the baseline, >= 0 */
    int   descent;      /* pixels below the baseline, <= 0 */
    int   line_gap;     /* extra pixels between lines, may be negative */
    int   used;
};

static struct mhnx_font g_fonts[MHNX_MAX_FONTS];

/* Handles are only ever pointers to g_fonts[] entries; anything else is
   rejected instead of dereferenced. */
static struct mhnx_font* mhnx_slot(void* f)
{
    int i;
    if (f == NULL)
        return NULL;
    for (i = 0; i < MHNX_MAX_FONTS; ++i) {
        if (g_fonts[i].used && (void*) &g_fonts[i] == f)
            return &g_fonts[i];
    }
    return NULL;
}

void* mhnx_font_create(const unsigned char* data, int len, float pixel_height)
{
    int i, offset, ascent, descent, line_gap;
    struct mhnx_font* s;

    if (data == NULL || len <= 0 || !(pixel_height > 0.0f))
        return NULL;

    for (i = 0; i < MHNX_MAX_FONTS; ++i) {
        if (!g_fonts[i].used)
            break;
    }
    if (i == MHNX_MAX_FONTS)
        return NULL;

    s = &g_fonts[i];
    memset(s, 0, sizeof(*s));

    offset = stbtt_GetFontOffsetForIndex(data, 0);
    if (offset < 0)
        return NULL;
    if (!stbtt_InitFont(&s->info, data, offset))
        return NULL;

    s->scale = stbtt_ScaleForPixelHeight(&s->info, pixel_height);
    stbtt_GetFontVMetrics(&s->info, &ascent, &descent, &line_gap);
    s->ascent   = mhnx_ifloor(ascent   * (double) s->scale + 0.5);
    s->descent  = mhnx_ifloor(descent  * (double) s->scale - 0.5);
    s->line_gap = mhnx_ifloor(line_gap * (double) s->scale + 0.5);
    s->used = 1;
    return s;
}

int mhnx_font_ascent(void* f)
{
    struct mhnx_font* s = mhnx_slot(f);
    return s ? s->ascent : 0;
}

int mhnx_font_descent(void* f)
{
    struct mhnx_font* s = mhnx_slot(f);
    return s ? s->descent : 0;
}

int mhnx_font_line_gap(void* f)
{
    struct mhnx_font* s = mhnx_slot(f);
    return s ? s->line_gap : 0;
}

/* 1 when the outlines are CFF (OpenType/PostScript) rather than TrueType
   'glyf'.  Diagnostic only - stb_truetype rasterises both. */
int mhnx_font_cff(void* f)
{
    struct mhnx_font* s = mhnx_slot(f);
    return s ? (s->info.cff.size != 0) : 0;
}

int mhnx_font_glyph(void* f, int codepoint, unsigned char* out, int out_cap,
                    int* w, int* h, int* xoff, int* yoff, int* advance)
{
    struct mhnx_font* s = mhnx_slot(f);
    int gi, adv, lsb, ix0, iy0, ix1, iy1, gw, gh;

    if (w) *w = 0;
    if (h) *h = 0;
    if (xoff) *xoff = 0;
    if (yoff) *yoff = 0;
    if (advance) *advance = 0;
    if (s == NULL)
        return -1;

    gi = stbtt_FindGlyphIndex(&s->info, codepoint);
    if (gi == 0 && codepoint != 0)
        return -2;

    stbtt_GetGlyphHMetrics(&s->info, gi, &adv, &lsb);
    if (advance)
        *advance = mhnx_ifloor(adv * (double) s->scale + 0.5);

    stbtt_GetGlyphBitmapBox(&s->info, gi, s->scale, s->scale, &ix0, &iy0, &ix1, &iy1);
    gw = ix1 - ix0;
    gh = iy1 - iy0;
    if (w) *w = gw > 0 ? gw : 0;
    if (h) *h = gh > 0 ? gh : 0;
    if (xoff) *xoff = ix0;
    if (yoff) *yoff = iy0;

    if (gw <= 0 || gh <= 0)
        return 0;                       /* blank glyph: nothing to rasterise */
    if (out == NULL || out_cap < gw * gh)
        return gw * gh;                 /* caller must allocate gw*gh bytes */

    stbtt_MakeGlyphBitmap(&s->info, out, gw, gh, gw, s->scale, s->scale, gi);
    return 0;
}

void mhnx_font_destroy(void* f)
{
    struct mhnx_font* s = mhnx_slot(f);
    if (s)
        memset(s, 0, sizeof(*s));
}
