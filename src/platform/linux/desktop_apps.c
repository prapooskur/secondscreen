#include <gio/gdesktopappinfo.h>

typedef void (*AppCallback)(void *, const char *, const char *, const char *);
typedef void (*ErrorCallback)(void *, const char *);

void secondscreen_desktop_apps(AppCallback callback, void *data)
{
    GList *apps = g_app_info_get_all();
    for (GList *item = apps; item; item = item->next) {
        GAppInfo *app = item->data;
        if (!G_IS_DESKTOP_APP_INFO(app) || !g_app_info_should_show(app))
            continue;

        char *icon = g_desktop_app_info_get_string(G_DESKTOP_APP_INFO(app), "Icon");
        callback(data, g_app_info_get_id(app), g_app_info_get_display_name(app), icon);
        g_free(icon);
    }
    g_list_free_full(apps, g_object_unref);
}

typedef struct {
    gboolean done;
    GError *error;
} LaunchResult;

static void launch_finished(GObject *source, GAsyncResult *result, gpointer data)
{
    LaunchResult *launch = data;
    g_app_info_launch_uris_finish(G_APP_INFO(source), result, &launch->error);
    launch->done = TRUE;
}

void secondscreen_launch_desktop_app(const char *id, ErrorCallback callback, void *data)
{
    GDesktopAppInfo *app = g_desktop_app_info_new(id);
    if (!app || g_desktop_app_info_get_is_hidden(app)) {
        callback(data, "The app is no longer installed.");
        g_clear_object(&app);
        return;
    }

    GMainContext *context = g_main_context_new();
    g_main_context_push_thread_default(context);
    LaunchResult launch = {0};
    g_app_info_launch_uris_async(G_APP_INFO(app), NULL, NULL, NULL, launch_finished, &launch);
    while (!launch.done)
        g_main_context_iteration(context, TRUE);

    if (launch.error) {
        callback(data, launch.error->message);
        g_error_free(launch.error);
    }
    g_main_context_pop_thread_default(context);
    g_main_context_unref(context);
    g_object_unref(app);
}
