#include <X11/Xatom.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/cursorfont.h>
#include <X11/extensions/XInput2.h>

#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <time.h>
#include <unistd.h>

static Display *dpy;
static Window root;

static Cursor cursors[8];
static Window cursor_window = None;

/*
 * Geometry is expressed in logical pixels and multiplied by Varmint's
 * host-derived UI scale at runtime.
 *
 * scale=2 reproduces the values used by the original proven helper:
 *   edge=8, corner=32, hotspot=48,
 *   drag_height=48, drag_left=320, drag_right=180.
 */
static int edge_size;
static int corner_size;
static int br_hotspot;
static int drag_height;
static int drag_left;
static int drag_right;

#define RAW_MOTION_INTERVAL_NS (33LL * 1000 * 1000)

static int raw_motion_check_due(struct timespec *last)
{
    struct timespec now;

    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0)
        return 1;

    long long delta =
        (long long)(now.tv_sec - last->tv_sec) * 1000000000LL +
        (long long)(now.tv_nsec - last->tv_nsec);

    if (delta < RAW_MOTION_INTERVAL_NS)
        return 0;

    *last = now;
    return 1;
}

static int read_ui_scale(void)
{
    FILE *f = fopen("/etc/varmint/ui-scale", "r");
    int scale = 1;

    if (f) {
        int value = 1;
        if (fscanf(f, "%d", &value) == 1 && (value == 1 || value == 2))
            scale = value;
        fclose(f);
    }

    return scale;
}

static void configure_geometry(void)
{
    int scale = read_ui_scale();

    edge_size  = 4   * scale;
    corner_size = 16 * scale;
    br_hotspot = 24  * scale;
    drag_height = 24 * scale;
    drag_left  = 160 * scale;
    drag_right = 90  * scale;

    fprintf(stderr,
        "steam-edge-resize running\n"
        "  scale: %d\n"
        "  edge: %d px\n"
        "  corners: %d px\n"
        "  bottom-right hotspot: %dx%d px\n"
        "  drag: y<%d, x=%d..(w-%d)\n",
        scale,
        edge_size,
        corner_size,
        br_hotspot,
        br_hotspot,
        drag_height,
        drag_left,
        drag_right);
}

static int x_error_handler(Display *display, XErrorEvent *e)
{
    char msg[256] = {0};

    XGetErrorText(display, e->error_code, msg, sizeof(msg));

    fprintf(stderr,
        "X11 warning: %s request=%u.%u resource=0x%lx\n",
        msg,
        e->request_code,
        e->minor_code,
        e->resourceid);

    /* CEF frequently destroys/recreates child windows. */
    return 0;
}

static int has_atom(Window w, Atom property, Atom wanted)
{
    Atom actual_type;
    int actual_format;
    unsigned long nitems, bytes_after;
    unsigned char *data = NULL;

    if (XGetWindowProperty(
            dpy, w, property,
            0, 1024,
            False, XA_ATOM,
            &actual_type, &actual_format,
            &nitems, &bytes_after,
            &data) != Success || !data)
        return 0;

    int found = 0;

    if (actual_type == XA_ATOM && actual_format == 32) {
        Atom *atoms = (Atom *)data;

        for (unsigned long i = 0; i < nitems; ++i) {
            if (atoms[i] == wanted) {
                found = 1;
                break;
            }
        }
    }

    XFree(data);
    return found;
}

static int blocked_state(Window w)
{
    Atom state =
        XInternAtom(dpy, "_NET_WM_STATE", False);

    Atom fullscreen =
        XInternAtom(dpy, "_NET_WM_STATE_FULLSCREEN", False);

    Atom max_h =
        XInternAtom(dpy, "_NET_WM_STATE_MAXIMIZED_HORZ", False);

    Atom max_v =
        XInternAtom(dpy, "_NET_WM_STATE_MAXIMIZED_VERT", False);

    if (has_atom(w, state, fullscreen))
        return 1;

    if (has_atom(w, state, max_h) &&
        has_atom(w, state, max_v))
        return 1;

    return 0;
}

static void find_steam_window(
    Window w,
    int rx,
    int ry,
    Window *best,
    long *best_area,
    int *best_x,
    int *best_y,
    int *best_w,
    int *best_h)
{
    XWindowAttributes a;

    if (!XGetWindowAttributes(dpy, w, &a))
        return;

    if (a.map_state == IsViewable && !a.override_redirect) {
        XClassHint ch = {0};

        if (XGetClassHint(dpy, w, &ch)) {
            int is_steam =
                ch.res_class &&
                !strcasecmp(ch.res_class, "steam");

            if (is_steam) {
                Window dummy;
                int wx = 0;
                int wy = 0;

                XTranslateCoordinates(
                    dpy,
                    w,
                    root,
                    0, 0,
                    &wx, &wy,
                    &dummy);

                if (rx >= wx &&
                    ry >= wy &&
                    rx < wx + a.width &&
                    ry < wy + a.height) {

                    long area =
                        (long)a.width *
                        (long)a.height;

                    if (area < *best_area) {
                        *best = w;
                        *best_area = area;
                        *best_x = wx;
                        *best_y = wy;
                        *best_w = a.width;
                        *best_h = a.height;
                    }
                }
            }

            if (ch.res_name)
                XFree(ch.res_name);

            if (ch.res_class)
                XFree(ch.res_class);
        }
    }

    Window rr, parent;
    Window *children = NULL;
    unsigned int nchildren = 0;

    if (!XQueryTree(
            dpy,
            w,
            &rr,
            &parent,
            &children,
            &nchildren))
        return;

    for (unsigned int i = 0; i < nchildren; ++i) {
        find_steam_window(
            children[i],
            rx, ry,
            best,
            best_area,
            best_x, best_y,
            best_w, best_h);
    }

    if (children)
        XFree(children);
}

/*
 * _NET_WM_MOVERESIZE directions:
 *
 * 0 TL
 * 1 T
 * 2 TR
 * 3 R
 * 4 BR
 * 5 B
 * 6 BL
 * 7 L
 * 8 MOVE
 */
static int edge_direction(int x, int y, int w, int h)
{
    if (x >= w - br_hotspot &&
        y >= h - br_hotspot)
        return 4;

    int left   = x < edge_size;
    int right  = x >= w - edge_size;
    int top    = y < edge_size;
    int bottom = y >= h - edge_size;

    if (!(left || right || top || bottom))
        return -1;

    if (top) {
        if (x < corner_size)
            return 0;

        if (x >= w - corner_size)
            return 2;

        return 1;
    }

    if (bottom) {
        if (x < corner_size)
            return 6;

        if (x >= w - corner_size)
            return 4;

        return 5;
    }

    if (left)
        return 7;

    return 3;
}

static int drag_region(int x, int y, int w)
{
    if (y < 0 || y >= drag_height)
        return 0;

    if (x < drag_left)
        return 0;

    if (x >= w - drag_right)
        return 0;

    return 1;
}

static int pointer_target(
    Window *win_out,
    int *rx_out,
    int *ry_out,
    int *lx_out,
    int *ly_out,
    int *w_out,
    int *h_out)
{
    Window rr, child;
    int rx, ry, wx, wy;
    unsigned int state;

    if (!XQueryPointer(
            dpy,
            root,
            &rr,
            &child,
            &rx,
            &ry,
            &wx,
            &wy,
            &state))
        return 0;

    Window win = None;
    long area = LONG_MAX;

    int x0 = 0;
    int y0 = 0;
    int w = 0;
    int h = 0;

    find_steam_window(
        root,
        rx, ry,
        &win,
        &area,
        &x0, &y0,
        &w, &h);

    if (win == None)
        return 0;

    if (blocked_state(win))
        return 0;

    *win_out = win;

    *rx_out = rx;
    *ry_out = ry;

    *lx_out = rx - x0;
    *ly_out = ry - y0;

    *w_out = w;
    *h_out = h;

    return 1;
}

static Window deepest_window_at_pointer(void)
{
    Window current = root;

    for (;;) {
        Window rr, child;
        int root_x, root_y;
        int win_x, win_y;
        unsigned int mask;

        if (!XQueryPointer(
                dpy,
                current,
                &rr,
                &child,
                &root_x,
                &root_y,
                &win_x,
                &win_y,
                &mask))
            break;

        if (child == None)
            break;

        current = child;
    }

    return current;
}

static void clear_resize_cursor(void)
{
    if (cursor_window != None) {
        XUndefineCursor(dpy, cursor_window);
        cursor_window = None;
        XFlush(dpy);
    }
}

static void set_resize_cursor(int direction)
{
    Window leaf =
        deepest_window_at_pointer();

    if (leaf == None)
        return;

    if (cursor_window != None &&
        cursor_window != leaf) {
        XUndefineCursor(
            dpy,
            cursor_window);
    }

    XDefineCursor(
        dpy,
        leaf,
        cursors[direction]);

    cursor_window = leaf;

    XFlush(dpy);
}

static void moveresize(
    Window win,
    int rx,
    int ry,
    int direction)
{
    Atom atom =
        XInternAtom(
            dpy,
            "_NET_WM_MOVERESIZE",
            False);

    XEvent e;
    memset(&e, 0, sizeof(e));

    e.xclient.type = ClientMessage;
    e.xclient.window = win;
    e.xclient.message_type = atom;
    e.xclient.format = 32;

    e.xclient.data.l[0] = rx;
    e.xclient.data.l[1] = ry;
    e.xclient.data.l[2] = direction;
    e.xclient.data.l[3] = 1;
    e.xclient.data.l[4] = 0;

    XUngrabPointer(
        dpy,
        CurrentTime);

    XSendEvent(
        dpy,
        root,
        False,
        SubstructureRedirectMask |
        SubstructureNotifyMask,
        &e);

    XFlush(dpy);
}

static int open_display(void)
{
    /*
     * Plasma Wayland may start the autostart entry just before Xwayland
     * becomes reachable. Give it a short bounded grace period.
     */
    for (int i = 0; i < 100; ++i) {
        dpy = XOpenDisplay(NULL);
        if (dpy)
            return 1;

        usleep(100000);
    }

    return 0;
}

int main(void)
{
    configure_geometry();

    if (!open_display()) {
        fprintf(stderr, "cannot open X display\n");
        return 1;
    }

    XSetErrorHandler(x_error_handler);

    root = DefaultRootWindow(dpy);

    cursors[0] =
        XCreateFontCursor(dpy, XC_top_left_corner);

    cursors[1] =
        XCreateFontCursor(dpy, XC_top_side);

    cursors[2] =
        XCreateFontCursor(dpy, XC_top_right_corner);

    cursors[3] =
        XCreateFontCursor(dpy, XC_right_side);

    cursors[4] =
        XCreateFontCursor(dpy, XC_bottom_right_corner);

    cursors[5] =
        XCreateFontCursor(dpy, XC_bottom_side);

    cursors[6] =
        XCreateFontCursor(dpy, XC_bottom_left_corner);

    cursors[7] =
        XCreateFontCursor(dpy, XC_left_side);

    int xi_opcode;
    int xi_event;
    int xi_error;

    if (!XQueryExtension(
            dpy,
            "XInputExtension",
            &xi_opcode,
            &xi_event,
            &xi_error)) {
        fprintf(stderr, "XInput2 unavailable\n");
        return 1;
    }

    int major = 2;
    int minor = 0;

    if (XIQueryVersion(
            dpy,
            &major,
            &minor) != Success) {
        fprintf(stderr, "XI2 negotiation failed\n");
        return 1;
    }

    unsigned char mask[
        XIMaskLen(XI_RawMotion)
    ];

    memset(mask, 0, sizeof(mask));

    XISetMask(
        mask,
        XI_RawMotion);

    XISetMask(
        mask,
        XI_RawButtonPress);

    XIEventMask em = {
        .deviceid = XIAllMasterDevices,
        .mask_len = sizeof(mask),
        .mask = mask,
    };

    XISelectEvents(
        dpy,
        root,
        &em,
        1);

    XFlush(dpy);

    struct timespec last_motion_check = {0};

    for (;;) {
        XEvent ev;
        XNextEvent(dpy, &ev);

        if (ev.xcookie.type != GenericEvent ||
            ev.xcookie.extension != xi_opcode ||
            !XGetEventData(
                dpy,
                &ev.xcookie))
            continue;

        if (ev.xcookie.evtype == XI_RawMotion &&
            raw_motion_check_due(&last_motion_check)) {
            Window win;
            int rx, ry;
            int lx, ly;
            int w, h;

            if (pointer_target(
                    &win,
                    &rx, &ry,
                    &lx, &ly,
                    &w, &h)) {

                int direction =
                    edge_direction(
                        lx, ly,
                        w, h);

                if (direction >= 0)
                    set_resize_cursor(direction);
                else
                    clear_resize_cursor();
            } else {
                clear_resize_cursor();
            }
        }

        if (ev.xcookie.evtype == XI_RawButtonPress) {
            XIRawEvent *raw =
                (XIRawEvent *)ev.xcookie.data;

            if (raw->detail == 1) {
                Window win;
                int rx, ry;
                int lx, ly;
                int w, h;

                if (pointer_target(
                        &win,
                        &rx, &ry,
                        &lx, &ly,
                        &w, &h)) {

                    int direction =
                        edge_direction(
                            lx, ly,
                            w, h);

                    if (direction >= 0) {
                        moveresize(
                            win,
                            rx, ry,
                            direction);
                    }
                    else if (drag_region(
                                 lx,
                                 ly,
                                 w)) {
                        moveresize(
                            win,
                            rx, ry,
                            8);
                    }
                }
            }
        }

        XFreeEventData(
            dpy,
            &ev.xcookie);
    }
}
