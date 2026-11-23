#include <dlfcn.h>
#include <webkit2/webkit2.h>

/* Resolve Viewer-specific calls from WebKitGTK and its dependencies on first use.
   Keep the library open for the lifetime of the GTK widgets and callbacks. */
#define WEBKIT_FUNCTIONS(X) \
    X(gdk_display_get_default_seat) \
    X(gdk_event_free) \
    X(gdk_event_new) \
    X(gdk_event_set_device) \
    X(gdk_keymap_get_entries_for_keyval) \
    X(gdk_keymap_get_for_display) \
    X(gdk_keyval_from_name) \
    X(gdk_offscreen_window_set_embedder) \
    X(gdk_seat_get_keyboard) \
    X(gdk_seat_get_pointer) \
    X(gdk_unicode_to_keyval) \
    X(gtk_container_add) \
    X(gtk_init_check) \
    X(gtk_main) \
    X(gtk_main_quit) \
    X(gtk_offscreen_window_new) \
    X(gtk_widget_destroy) \
    X(gtk_widget_draw) \
    X(gtk_widget_get_display) \
    X(gtk_widget_get_preferred_size) \
    X(gtk_widget_get_window) \
    X(gtk_widget_grab_focus) \
    X(gtk_widget_map) \
    X(gtk_widget_realize) \
    X(gtk_widget_send_focus_change) \
    X(gtk_widget_set_parent_window) \
    X(gtk_widget_show) \
    X(gtk_widget_show_all) \
    X(gtk_widget_size_allocate) \
    X(gtk_window_new) \
    X(gtk_window_set_default_size) \
    X(gtk_window_set_title) \
    X(webkit_navigation_action_get_request) \
    X(webkit_navigation_policy_decision_get_navigation_action) \
    X(webkit_policy_decision_ignore) \
    X(webkit_settings_set_enable_fullscreen) \
    X(webkit_settings_set_hardware_acceleration_policy) \
    X(webkit_settings_set_media_playback_allows_inline) \
    X(webkit_uri_request_get_uri) \
    X(webkit_web_context_new_ephemeral) \
    X(webkit_web_view_get_settings) \
    X(webkit_web_view_load_html) \
    X(webkit_web_view_load_uri) \
    X(webkit_web_view_new_with_context)

static struct {
#define DECLARE_SYMBOL(name) __typeof__(&name) name;
    WEBKIT_FUNCTIONS(DECLARE_SYMBOL)
#undef DECLARE_SYMBOL
} webkit_api;

const char *ss_web_load(void) {
    static void *library;
    if (library)
        return NULL;

    void *handle = dlopen("libwebkit2gtk-4.1.so.0", RTLD_NOW | RTLD_LOCAL);
    if (!handle)
        return dlerror();

#define LOAD_SYMBOL(name)                                      \
    webkit_api.name = (__typeof__(&name))dlsym(handle, #name);  \
    if (!webkit_api.name) {                                    \
        dlclose(handle);                                       \
        return "Missing WebKitGTK symbol: " #name;             \
    }
    WEBKIT_FUNCTIONS(LOAD_SYMBOL)
#undef LOAD_SYMBOL

    library = handle;
    return NULL;
}

#undef WEBKIT_FUNCTIONS

typedef struct {
    GtkWidget *window;
    GtkWidget *target;
    GtkWidget *web;
} TileView;

static void allocate_view(TileView *tile, int width, int height) {
    GtkAllocation bounds = {0, 0, width, height};
    GtkRequisition minimum;
    webkit_api.gtk_widget_get_preferred_size(tile->web, &minimum, NULL);
    webkit_api.gtk_widget_size_allocate(tile->web, &bounds);
}

/* Keep navigation in the web sandbox. External applications are opened by the
   dashboard's explicit Browser and Discord actions. */
static gboolean navigation(WebKitWebView *web G_GNUC_UNUSED, WebKitPolicyDecision *decision,
                           WebKitPolicyDecisionType type, gpointer data) {
    if (type != WEBKIT_POLICY_DECISION_TYPE_NAVIGATION_ACTION &&
        type != WEBKIT_POLICY_DECISION_TYPE_NEW_WINDOW_ACTION)
        return FALSE;
    WebKitNavigationAction *action = webkit_api.webkit_navigation_policy_decision_get_navigation_action(
        (WebKitNavigationPolicyDecision *)decision);
    const char *uri = webkit_api.webkit_uri_request_get_uri(webkit_api.webkit_navigation_action_get_request(action));
    if (type == WEBKIT_POLICY_DECISION_TYPE_NEW_WINDOW_ACTION ||
        !(g_str_has_prefix(uri, "https://") || g_str_has_prefix(uri, "http://") ||
          g_str_equal(uri, "about:blank") || (data && g_str_has_prefix(uri, "file://")))) {
        webkit_api.webkit_policy_decision_ignore(decision);
        return TRUE;
    }
    return FALSE;
}

TileView *ss_web_new(const char *uri, const char *html, int width, int height, int standalone) {
    if (!webkit_api.gtk_init_check(NULL, NULL))
        return NULL;
    TileView *tile = g_new0(TileView, 1);
    tile->window = webkit_api.gtk_window_new(GTK_WINDOW_TOPLEVEL);
    WebKitWebContext *context = webkit_api.webkit_web_context_new_ephemeral();
    tile->web = webkit_api.webkit_web_view_new_with_context(context);
    g_object_unref(context);
    WebKitWebView *web = (WebKitWebView *)tile->web;
    WebKitSettings *settings = webkit_api.webkit_web_view_get_settings(web);
    if (!standalone) {
        webkit_api.webkit_settings_set_media_playback_allows_inline(settings, TRUE);
        /* The tile draws to Cairo. */
        webkit_api.webkit_settings_set_hardware_acceleration_policy(settings, WEBKIT_HARDWARE_ACCELERATION_POLICY_NEVER);
        webkit_api.webkit_settings_set_enable_fullscreen(settings, FALSE);
    }
    g_signal_connect(web, "decide-policy", G_CALLBACK(navigation),
                     GINT_TO_POINTER(html && g_str_has_prefix(uri, "file://")));
    webkit_api.gtk_window_set_title((GtkWindow *)tile->window, "Second Screen Viewer");
    webkit_api.gtk_window_set_default_size((GtkWindow *)tile->window, width, height);
    if (standalone) {
        webkit_api.gtk_container_add((GtkContainer *)tile->window, tile->web);
        webkit_api.gtk_widget_show_all(tile->window);
    } else {
        /* WebKit starts media once it has a GtkWindow ancestor. Keep that host
           unmapped and paint its child into the dashboard's offscreen target. */
        webkit_api.gtk_widget_realize(tile->window);
        tile->target = webkit_api.gtk_offscreen_window_new();
        webkit_api.gtk_window_set_default_size((GtkWindow *)tile->target, width, height);
        webkit_api.gtk_widget_show(tile->target);
        GdkWindow *surface = webkit_api.gtk_widget_get_window(tile->target);
        webkit_api.gdk_offscreen_window_set_embedder(surface, webkit_api.gtk_widget_get_window(tile->window));
        webkit_api.gtk_container_add((GtkContainer *)tile->window, tile->web);
        webkit_api.gtk_widget_set_parent_window(tile->web, surface);
        webkit_api.gtk_widget_show(tile->web);
        webkit_api.gtk_widget_realize(tile->web);
        allocate_view(tile, width, height);
        webkit_api.gtk_widget_map(tile->web);
    }
    webkit_api.gtk_widget_grab_focus(tile->web);
    if (html)
        webkit_api.webkit_web_view_load_html(web, html, uri);
    else
        webkit_api.webkit_web_view_load_uri(web, uri);
    return tile;
}

void ss_web_free(TileView *tile) {
    webkit_api.gdk_offscreen_window_set_embedder(webkit_api.gtk_widget_get_window(tile->target), NULL);
    webkit_api.gtk_widget_destroy(tile->window);
    webkit_api.gtk_widget_destroy(tile->target);
    g_free(tile);
}

void ss_web_run(TileView *tile) {
    g_signal_connect(tile->window, "destroy", G_CALLBACK(webkit_api.gtk_main_quit), NULL);
    webkit_api.gtk_main();
    g_free(tile);
}

int ss_web_has_discord(void) {
    GAppInfo *app = g_app_info_get_default_for_uri_scheme("discord");
    if (!app)
        return FALSE;
    g_object_unref(app);
    return TRUE;
}

void ss_web_pump(void) {
    /* GPUI owns the UI thread. Return to its event loop between GTK batches. */
    for (int i = 0; i < 32 && g_main_context_iteration(NULL, FALSE); i++) {}
}

void ss_web_resize(TileView *tile, int width, int height) {
    webkit_api.gtk_window_set_default_size((GtkWindow *)tile->target, width, height);
    allocate_view(tile, width, height);
}

int ss_web_frame(TileView *tile, unsigned char *bgra, int width, int height) {
    allocate_view(tile, width, height);
    cairo_surface_t *dst = cairo_image_surface_create_for_data(bgra, CAIRO_FORMAT_ARGB32,
                                                               width, height, width * 4);
    cairo_t *cr = cairo_create(dst);
    cairo_set_operator(cr, CAIRO_OPERATOR_CLEAR);
    cairo_paint(cr);
    cairo_set_operator(cr, CAIRO_OPERATOR_OVER);
    webkit_api.gtk_widget_draw(tile->web, cr);
    cairo_destroy(cr);
    cairo_surface_flush(dst);
    gboolean ok = cairo_surface_status(dst) == CAIRO_STATUS_SUCCESS;
    cairo_surface_destroy(dst);
    return ok;
}

static GdkEvent *event_new(TileView *tile, GdkEventType type, gboolean keyboard) {
    GdkEvent *event = webkit_api.gdk_event_new(type);
    event->any.window = g_object_ref(webkit_api.gtk_widget_get_window(tile->web));
    GdkSeat *seat = webkit_api.gdk_display_get_default_seat(webkit_api.gtk_widget_get_display(tile->web));
    webkit_api.gdk_event_set_device(event, keyboard ? webkit_api.gdk_seat_get_keyboard(seat) : webkit_api.gdk_seat_get_pointer(seat));
    return event;
}

void ss_web_pointer(TileView *tile, int action, int button, double x, double y,
                    double dx, double dy, unsigned int state) {
    GdkEventType types[] = {GDK_MOTION_NOTIFY, GDK_BUTTON_PRESS, GDK_BUTTON_RELEASE,
                            GDK_SCROLL, GDK_2BUTTON_PRESS, GDK_3BUTTON_PRESS};
    GdkEvent *e = event_new(tile, types[action], FALSE);
    guint32 time = (guint32)(g_get_monotonic_time() / 1000);
    if (action == 0) {
        e->motion.x = x; e->motion.y = y; e->motion.time = time; e->motion.state = state;
    } else if (action == 3) {
        e->scroll.x = x; e->scroll.y = y; e->scroll.time = time; e->scroll.state = state;
        e->scroll.direction = GDK_SCROLL_SMOOTH;
        e->scroll.delta_x = dx; e->scroll.delta_y = dy;
    } else {
        e->button.x = x; e->button.y = y; e->button.time = time;
        e->button.state = state; e->button.button = button;
    }
    /* gtk_widget_event drops input to unmapped native windows. GPUI has
       already hit-tested the on-screen tile, so dispatch to the web widget. */
    const char *signals[] = {"motion-notify-event", "button-press-event", "button-release-event",
                             "scroll-event", "button-press-event", "button-press-event"};
    gboolean handled;
    g_signal_emit_by_name(tile->web, signals[action], e, &handled);
    webkit_api.gdk_event_free(e);
}

void ss_web_focus(TileView *tile, int focus) {
    GdkEvent *e = event_new(tile, GDK_FOCUS_CHANGE, TRUE);
    e->focus_change.in = focus;
    webkit_api.gtk_widget_send_focus_change(tile->web, e);
    webkit_api.gdk_event_free(e);
}

void ss_web_key(TileView *tile, const char *name, unsigned int unicode,
                unsigned int state, int press) {
    guint key = unicode ? webkit_api.gdk_unicode_to_keyval(unicode) : webkit_api.gdk_keyval_from_name(name);
    if (!key || key == GDK_KEY_VoidSymbol)
        return;
    GdkEvent *e = event_new(tile, press ? GDK_KEY_PRESS : GDK_KEY_RELEASE, TRUE);
    e->key.keyval = key;
    e->key.state = state;
    e->key.time = (guint32)(g_get_monotonic_time() / 1000);
    GdkKeymapKey *keys = NULL;
    int count = 0;
    if (webkit_api.gdk_keymap_get_entries_for_keyval(webkit_api.gdk_keymap_get_for_display(webkit_api.gtk_widget_get_display(tile->web)), key, &keys, &count)) {
        e->key.hardware_keycode = keys[0].keycode;
        e->key.group = keys[0].group;
        g_free(keys);
    }
    gboolean handled;
    g_signal_emit_by_name(tile->web, press ? "key-press-event" : "key-release-event", e, &handled);
    webkit_api.gdk_event_free(e);
}
