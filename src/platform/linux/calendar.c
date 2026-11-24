#include <libecal/libecal.h>
#include <stdint.h>

typedef void (*EventCallback)(void *, const char *, int64_t, int64_t, int);

typedef struct {
    EventCallback callback;
    void *data;
    ICalTimezone *zone;
} EventSink;

static time_t event_time(ICalTime *time, ICalTimezone *local_zone, gboolean all_day)
{
    // Encode date-only values at midnight UTC so they retain their original date.
    if (all_day)
        return i_cal_time_as_timet_with_zone(time, i_cal_timezone_get_utc_timezone());
    ICalTimezone *zone = i_cal_time_get_timezone(time);
    return i_cal_time_as_timet_with_zone(time, zone ? zone : local_zone);
}

static gboolean collect_event(ICalComponent *component, ICalTime *start, ICalTime *end,
    gpointer data, G_GNUC_UNUSED GCancellable *cancellable, G_GNUC_UNUSED GError **error)
{
    if (i_cal_component_get_status(component) == I_CAL_STATUS_CANCELLED)
        return TRUE;

    EventSink *sink = data;
    gboolean all_day = i_cal_time_is_date(start);
    sink->callback(sink->data, i_cal_component_get_summary(component),
        event_time(start, sink->zone, all_day), event_time(end, sink->zone, all_day), all_day);
    return TRUE;
}

// Returns 0 on success, 1 when some calendars failed, and -1 if the feed is unavailable.
int secondscreen_calendar_events(int64_t start, int64_t end, const char *timezone,
    EventCallback callback, void *data)
{
    GMainContext *context = g_main_context_new();
    g_main_context_push_thread_default(context);
    ESourceRegistry *registry = e_source_registry_new_sync(NULL, NULL);
    int status = -1;
    if (registry) {
        ICalTimezone *zone = timezone ? i_cal_timezone_get_builtin_timezone(timezone) : NULL;
        if (!zone)
            zone = e_cal_util_get_system_timezone();
        EventSink sink = {callback, data, zone ? zone : i_cal_timezone_get_utc_timezone()};
        GList *sources = e_source_registry_list_enabled(registry, E_SOURCE_EXTENSION_CALENDAR);
        gboolean loaded = FALSE;
        gboolean failed = FALSE;
        for (GList *item = sources; item; item = item->next) {
            ESource *source = item->data;
            ESourceSelectable *calendar = e_source_get_extension(source, E_SOURCE_EXTENSION_CALENDAR);
            if (!e_source_selectable_get_selected(calendar))
                continue;

            EClient *client = e_cal_client_connect_sync(source, E_CAL_CLIENT_SOURCE_TYPE_EVENTS,
                (guint32)-1, NULL, NULL);
            if (!client) {
                failed = TRUE;
                continue;
            }
            ECalClient *events = E_CAL_CLIENT(client);
            e_cal_client_set_default_timezone(events, sink.zone);
            e_cal_client_generate_instances_sync(events, start, end, NULL, collect_event, &sink);
            loaded = TRUE;
            g_object_unref(client);
        }
        status = failed ? (loaded ? 1 : -1) : 0;
        g_list_free_full(sources, g_object_unref);
        g_object_unref(registry);
    }
    // Dispatch queued EDS notifications before releasing the thread's context.
    while (g_main_context_pending(context))
        g_main_context_iteration(context, FALSE);
    g_main_context_pop_thread_default(context);
    g_main_context_unref(context);
    return status;
}
