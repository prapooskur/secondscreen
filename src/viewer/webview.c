#include <webkit2/webkit2.h>


typedef struct {
    GtkWidget *window;
    GtkWidget *target;
    GtkWidget *web;
    gboolean dirty;
} TileView;

static void invalidated(GdkWindow *surface, cairo_region_t *region G_GNUC_UNUSED) {
    TileView *tile = g_object_get_data(G_OBJECT(surface), "tile-view");
    tile->dirty = TRUE;
}

static void allocate_view(TileView *tile, int width, int height) {
    GtkAllocation bounds = {0, 0, width, height};
    GtkRequisition minimum;
    gtk_widget_get_preferred_size(tile->web, &minimum, NULL);
    gtk_widget_size_allocate(tile->web, &bounds);
}

/* Keep navigation in the web sandbox. External applications are opened by the
   dashboard's explicit Browser and Discord actions. */
static gboolean navigation(WebKitWebView *web G_GNUC_UNUSED, WebKitPolicyDecision *decision,
                           WebKitPolicyDecisionType type, gpointer data) {
    if (type != WEBKIT_POLICY_DECISION_TYPE_NAVIGATION_ACTION &&
        type != WEBKIT_POLICY_DECISION_TYPE_NEW_WINDOW_ACTION)
        return FALSE;
    WebKitNavigationAction *action = webkit_navigation_policy_decision_get_navigation_action(
        WEBKIT_NAVIGATION_POLICY_DECISION(decision));
    const char *uri = webkit_uri_request_get_uri(webkit_navigation_action_get_request(action));
    if (type == WEBKIT_POLICY_DECISION_TYPE_NEW_WINDOW_ACTION ||
        !(g_str_has_prefix(uri, "https://") || g_str_has_prefix(uri, "http://") ||
          g_str_equal(uri, "about:blank") || (data && g_str_has_prefix(uri, "file://")))) {
        webkit_policy_decision_ignore(decision);
        return TRUE;
    }
    return FALSE;
}

TileView *ss_web_new(const char *uri, const char *html, int width, int height, int standalone) {
    if (!gtk_init_check(NULL, NULL))
        return NULL;
    TileView *tile = g_new0(TileView, 1);
    tile->window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    WebKitWebContext *context = webkit_web_context_new_ephemeral();
    tile->web = webkit_web_view_new_with_context(context);
    g_object_unref(context);
    WebKitWebView *web = WEBKIT_WEB_VIEW(tile->web);
    WebKitSettings *settings = webkit_web_view_get_settings(web);
    if (!standalone) {
        webkit_settings_set_media_playback_allows_inline(settings, TRUE);
        /* The tile draws to Cairo. */
        webkit_settings_set_hardware_acceleration_policy(settings, WEBKIT_HARDWARE_ACCELERATION_POLICY_NEVER);
        webkit_settings_set_enable_fullscreen(settings, FALSE);
    }
    g_signal_connect(web, "decide-policy", G_CALLBACK(navigation),
                     GINT_TO_POINTER(html && g_str_has_prefix(uri, "file://")));
    gtk_window_set_title(GTK_WINDOW(tile->window), "Second Screen Viewer");
    gtk_window_set_default_size(GTK_WINDOW(tile->window), width, height);
    if (standalone) {
        gtk_container_add(GTK_CONTAINER(tile->window), tile->web);
        gtk_widget_show_all(tile->window);
    } else {
        /* WebKit starts media once it has a GtkWindow ancestor. Keep that host
           unmapped and paint its child into the dashboard's offscreen target. */
        gtk_widget_realize(tile->window);
        tile->target = gtk_offscreen_window_new();
        gtk_window_set_default_size(GTK_WINDOW(tile->target), width, height);
        gtk_widget_show(tile->target);
        GdkWindow *surface = gtk_widget_get_window(tile->target);
        gdk_offscreen_window_set_embedder(surface, gtk_widget_get_window(tile->window));
        gtk_container_add(GTK_CONTAINER(tile->window), tile->web);
        gtk_widget_set_parent_window(tile->web, surface);
        gtk_widget_show(tile->web);
        gtk_widget_realize(tile->web);
        allocate_view(tile, width, height);
        gtk_widget_map(tile->web);
        GdkWindow *child = gtk_widget_get_window(tile->web);
        g_object_set_data(G_OBJECT(child), "tile-view", tile);
        gdk_window_set_invalidate_handler(child, invalidated);
        tile->dirty = TRUE;
    }
    gtk_widget_grab_focus(tile->web);
    if (html)
        webkit_web_view_load_html(web, html, uri);
    else
        webkit_web_view_load_uri(web, uri);
    return tile;
}

void ss_web_free(TileView *tile) {
    gdk_window_set_invalidate_handler(gtk_widget_get_window(tile->web), NULL);
    gdk_offscreen_window_set_embedder(gtk_widget_get_window(tile->target), NULL);
    gtk_widget_destroy(tile->window);
    gtk_widget_destroy(tile->target);
    g_free(tile);
}

void ss_web_run(TileView *tile) {
    g_signal_connect(tile->window, "destroy", G_CALLBACK(gtk_main_quit), NULL);
    gtk_main();
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
    gtk_window_set_default_size(GTK_WINDOW(tile->target), width, height);
    allocate_view(tile, width, height);
    tile->dirty = TRUE;
}

int ss_web_dirty(TileView *tile) { return tile->dirty; }

int ss_web_frame(TileView *tile, unsigned char *bgra, int width, int height) {
    allocate_view(tile, width, height);
    cairo_surface_t *dst = cairo_image_surface_create_for_data(bgra, CAIRO_FORMAT_ARGB32,
                                                               width, height, width * 4);
    cairo_t *cr = cairo_create(dst);
    gtk_widget_draw(tile->web, cr);
    cairo_destroy(cr);
    cairo_surface_flush(dst);
    gboolean ok = cairo_surface_status(dst) == CAIRO_STATUS_SUCCESS;
    cairo_surface_destroy(dst);
    tile->dirty = FALSE;
    return ok;
}

static GdkEvent *event_new(TileView *tile, GdkEventType type, gboolean keyboard) {
    GdkEvent *event = gdk_event_new(type);
    event->any.window = g_object_ref(gtk_widget_get_window(tile->web));
    GdkSeat *seat = gdk_display_get_default_seat(gtk_widget_get_display(tile->web));
    gdk_event_set_device(event, keyboard ? gdk_seat_get_keyboard(seat) : gdk_seat_get_pointer(seat));
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
    gtk_widget_event(tile->web, e);
    gdk_event_free(e);
}

void ss_web_focus(TileView *tile, int focus) {
    GdkEvent *e = event_new(tile, GDK_FOCUS_CHANGE, TRUE);
    e->focus_change.in = focus;
    gtk_widget_send_focus_change(tile->web, e);
    gdk_event_free(e);
}

void ss_web_key(TileView *tile, const char *name, unsigned int unicode,
                unsigned int state, int press) {
    guint key = unicode ? gdk_unicode_to_keyval(unicode) : gdk_keyval_from_name(name);
    if (!key || key == GDK_KEY_VoidSymbol)
        return;
    GdkEvent *e = event_new(tile, press ? GDK_KEY_PRESS : GDK_KEY_RELEASE, TRUE);
    e->key.keyval = key;
    e->key.state = state;
    e->key.time = (guint32)(g_get_monotonic_time() / 1000);
    GdkKeymapKey *keys = NULL;
    int count = 0;
    if (gdk_keymap_get_entries_for_keyval(gdk_keymap_get_for_display(gtk_widget_get_display(tile->web)), key, &keys, &count)) {
        e->key.hardware_keycode = keys[0].keycode;
        e->key.group = keys[0].group;
        g_free(keys);
    }
    gtk_widget_event(tile->web, e);
    gdk_event_free(e);
}
