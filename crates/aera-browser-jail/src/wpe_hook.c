/*
 * WPE policy hook for Skyward's AERA jail.
 *
 * Derived from the skyward-mikona implementation (GPL-3.0): redirect
 * target="_blank" / window.open navigations into the current view, and ask
 * WebKit to download responses whose MIME type it cannot render.
 */
#define RTLD_NEXT ((void *)-1L)

extern void *dlsym(void *handle, const char *symbol);

static int str_eq(const char *a, const char *b) {
    if (!a || !b) return 0;
    while (*a && *a == *b) {
        a++;
        b++;
    }
    return *a == *b;
}

static int starts_with(const char *s, const char *prefix) {
    if (!s || !prefix) return 0;
    while (*prefix) {
        if (*s++ != *prefix++) return 0;
    }
    return 1;
}

static int allowed_uri(const char *uri) {
    return starts_with(uri, "http://") || starts_with(uri, "https://") || starts_with(uri, "aera://");
}

typedef int (*decide_fn)(void *view, void *decision, int type, void *user_data);

static decide_fn orig_decide = 0;
static void *(*p_nav_get_action)(void *) = 0;
static void *(*p_act_get_req)(void *) = 0;
static const char *(*p_req_get_uri)(void *) = 0;
static void (*p_load_uri)(void *, const char *) = 0;
static void (*p_policy_ignore)(void *) = 0;
static void (*p_policy_download)(void *) = 0;
static int (*p_resp_mime_ok)(void *) = 0;
static unsigned long (*p_g_signal_connect_data)(void *, const char *, void *, void *, void *, int) = 0;

static void init_syms(void) {
    if (p_policy_ignore) return;
    p_nav_get_action = dlsym(RTLD_NEXT, "webkit_navigation_policy_decision_get_navigation_action");
    p_act_get_req = dlsym(RTLD_NEXT, "webkit_navigation_action_get_request");
    p_req_get_uri = dlsym(RTLD_NEXT, "webkit_uri_request_get_uri");
    p_load_uri = dlsym(RTLD_NEXT, "webkit_web_view_load_uri");
    p_policy_ignore = dlsym(RTLD_NEXT, "webkit_policy_decision_ignore");
    p_policy_download = dlsym(RTLD_NEXT, "webkit_policy_decision_download");
    p_resp_mime_ok = dlsym(RTLD_NEXT, "webkit_response_policy_decision_is_mime_type_supported");
}

static int hook_decide_policy(void *view, void *decision, int type, void *user_data) {
    init_syms();
    /* Redirect target="_blank" / window.open navigations into the single viewport. */
    if (type == 1 && p_nav_get_action && p_act_get_req && p_req_get_uri && p_load_uri && p_policy_ignore) {
        void *act = p_nav_get_action(decision);
        void *req = act ? p_act_get_req(act) : 0;
        const char *uri = req ? p_req_get_uri(req) : 0;
        if (uri && allowed_uri(uri)) {
            p_load_uri(view, uri);
        }
        p_policy_ignore(decision);
        return 1;
    }
    /* Download non-renderable binary responses (e.g. .zip, .img, .apk) even without Content-Disposition. */
    if (type == 2 && p_resp_mime_ok && p_policy_download) {
        if (!p_resp_mime_ok(decision)) {
            p_policy_download(decision);
            return 1;
        }
    }
    return orig_decide ? orig_decide(view, decision, type, user_data) : 0;
}

unsigned long g_signal_connect_data(
    void *instance,
    const char *detailed_signal,
    void *c_handler,
    void *data,
    void *destroy_data,
    int connect_flags
) {
    if (!p_g_signal_connect_data) {
        p_g_signal_connect_data = dlsym(RTLD_NEXT, "g_signal_connect_data");
    }
    if (!p_g_signal_connect_data) return 0;
    if (str_eq(detailed_signal, "decide-policy") && c_handler && c_handler != (void *)hook_decide_policy) {
        orig_decide = (decide_fn)c_handler;
        c_handler = (void *)hook_decide_policy;
    }
    return p_g_signal_connect_data(instance, detailed_signal, c_handler, data, destroy_data, connect_flags);
}
