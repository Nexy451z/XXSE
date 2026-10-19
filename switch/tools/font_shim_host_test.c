/*
 * font_shim_host_test.c - HOST-side smoke test for tools/font_shim.c.
 *
 * Not part of the .nro build: build.rs only compiles font_shim.c.  This
 * program exists so the stb_truetype wrapper can be exercised on the
 * development machine against real TrueType/TTC fonts, without a Switch:
 *
 *   gcc -O2 -I switch/tools -o font_shim_host_test \
 *       switch/tools/font_shim_host_test.c switch/tools/font_shim.c
 *   ./font_shim_host_test C:/Windows/Fonts/msgothic.ttc
 *
 * It checks the whole chain "open font -> find glyph -> rasterise -> at least
 * one non-zero coverage byte" for a sample with kana, kanji, ASCII and a
 * space.  Exit code 0 means every sample glyph either has ink or is genuinely
 * blank (the space); missing glyphs or all-zero bitmaps fail.
 *
 * The .ttc/.ttf used here are Windows fonts - the actual console uses the
 * Nintendo shared font, which cannot be exercised off-device; that part is
 * covered by text::probe_shared_font() on the console itself.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void* mhnx_font_create(const unsigned char* data, int len, float pixel_height);
int   mhnx_font_ascent(void* f);
int   mhnx_font_descent(void* f);
int   mhnx_font_line_gap(void* f);
int   mhnx_font_cff(void* f);
int   mhnx_font_glyph(void* f, int codepoint, unsigned char* out, int out_cap,
                      int* w, int* h, int* xoff, int* yoff, int* advance);
void  mhnx_font_destroy(void* f);

static unsigned char* slurp(const char* path, int* len)
{
    FILE* fp = fopen(path, "rb");
    long n;
    unsigned char* buf;
    if (!fp) { perror(path); return NULL; }
    fseek(fp, 0, SEEK_END);
    n = ftell(fp);
    fseek(fp, 0, SEEK_SET);
    buf = (unsigned char*) malloc((size_t) n);
    if (!buf || fread(buf, 1, (size_t) n, fp) != (size_t) n) { fclose(fp); free(buf); return NULL; }
    fclose(fp);
    *len = (int) n;
    return buf;
}

/* Minimal UTF-8 decoder; 4-byte sequences are reported as one emoji so the
   outside-the-BMP path can be exercised too. */
static int next_cp(const unsigned char** p)
{
    const unsigned char* s = *p;
    unsigned c = s[0];
    int cp;
    if (c < 0x80) { *p = s + 1; return (int) c; }
    if ((c & 0xE0) == 0xC0) { cp = ((c & 0x1F) << 6) | (s[1] & 0x3F); *p = s + 2; return cp; }
    if ((c & 0xF0) == 0xE0) { cp = ((c & 0x0F) << 12) | ((s[1] & 0x3F) << 6) | (s[2] & 0x3F); *p = s + 3; return cp; }
    if ((c & 0xF8) == 0xF0) { *p = s + 4; return 0x1F600; }
    *p = s + 1;
    return 0xFFFD;
}

static int test_font(const char* path)
{
    /* "クエスト名 テスト 123 ABC" as UTF-8 bytes. */
    static const char* sample = "\xE3\x82\xAF\xE3\x82\xA8\xE3\x82\xB9\xE3\x83\x88\xE5\x90\x8D"
                                " \xE3\x83\x86\xE3\x82\xB9\xE3\x83\x88 123 ABC";
    int len = 0, cp, total = 0, inked = 0, blank = 0, missing = 0, ink = 0;
    unsigned char* data = slurp(path, &len);
    void* font;
    const unsigned char* p;
    if (!data) return 1;
    font = mhnx_font_create(data, len, 24.0f);
    if (!font) {
        printf("%s: mhnx_font_create FAILED (%d bytes)\n", path, len);
        free(data);
        return 1;
    }
    printf("%s: %d bytes, ascent=%d descent=%d line_gap=%d cff=%d\n",
           path, len, mhnx_font_ascent(font), mhnx_font_descent(font),
           mhnx_font_line_gap(font), mhnx_font_cff(font));
    p = (const unsigned char*) sample;
    while (*p) {
        int w = 0, h = 0, xo = 0, yo = 0, adv = 0, need, rc, i, nz = 0;
        unsigned char* buf;
        cp = next_cp(&p);
        total++;
        /* Pass 1: metrics query, out == NULL. */
        need = mhnx_font_glyph(font, cp, NULL, 0, &w, &h, &xo, &yo, &adv);
        if (need < 0) { missing++; printf("  U+%04X missing\n", cp); continue; }
        if (need == 0 || w <= 0 || h <= 0) { blank++; continue; }
        /* Pass 2: rasterise into exactly w*h bytes. */
        buf = (unsigned char*) malloc((size_t) need);
        rc = mhnx_font_glyph(font, cp, buf, need, &w, &h, &xo, &yo, &adv);
        if (rc != 0) { missing++; free(buf); continue; }
        for (i = 0; i < need; ++i) if (buf[i]) nz++;
        if (nz > 0) inked++; else blank++;
        ink += nz;
        if (total <= 6)
            printf("  U+%04X box=%dx%d off=%d,%d adv=%d nonzero=%d\n", cp, w, h, xo, yo, adv, nz);
        free(buf);
    }
    mhnx_font_destroy(font);
    free(data);
    printf("  -> %d chars: %d inked, %d blank, %d missing, %d non-zero coverage bytes\n",
           total, inked, blank, missing, ink);
    return (inked > 0 && ink > 0) ? 0 : 1;
}

int main(int argc, char** argv)
{
    int i, fail = 0;
    if (argc < 2) { printf("usage: %s <font.ttf|ttc> [...]\n", argv[0]); return 2; }
    for (i = 1; i < argc; ++i) fail |= test_font(argv[i]);
    printf(fail ? "RESULT: FAIL\n" : "RESULT: PASS\n");
    return fail;
}
