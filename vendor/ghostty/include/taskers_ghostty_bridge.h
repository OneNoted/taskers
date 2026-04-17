#ifndef TASKERS_GHOSTTY_BRIDGE_H
#define TASKERS_GHOSTTY_BRIDGE_H

/*
 * Legacy compatibility shim for Taskers' historical GTK bridge surface.
 *
 * New consumers should prefer ghostty_gtk.h and the ghostty_gtk_* symbol
 * family. This header remains installed so existing runtime consumers can
 * keep building while the Rust side migrates to the generic GTK surface.
 */

#include "ghostty_gtk.h"

typedef ghostty_gtk_host_t taskers_ghostty_host_t;
typedef ghostty_gtk_host_options_s taskers_ghostty_host_options_s;
typedef ghostty_gtk_surface_options_s taskers_ghostty_surface_options_s;
typedef ghostty_gtk_text_s taskers_ghostty_text_s;

#define taskers_ghostty_host_new ghostty_gtk_host_new
#define taskers_ghostty_host_free ghostty_gtk_host_free
#define taskers_ghostty_host_version ghostty_gtk_host_version
#define taskers_ghostty_host_build_id ghostty_gtk_host_build_id
#define taskers_ghostty_host_begin_shutdown ghostty_gtk_host_begin_shutdown
#define taskers_ghostty_host_surface_count ghostty_gtk_host_surface_count
#define taskers_ghostty_host_tick ghostty_gtk_host_tick
#define taskers_ghostty_surface_new ghostty_gtk_surface_new
#define taskers_ghostty_surface_destroy ghostty_gtk_surface_destroy
#define taskers_ghostty_surface_grab_focus ghostty_gtk_surface_grab_focus
#define taskers_ghostty_surface_has_selection ghostty_gtk_surface_has_selection
#define taskers_ghostty_surface_send_text ghostty_gtk_surface_send_text
#define taskers_ghostty_surface_read_all_text ghostty_gtk_surface_read_all_text
#define taskers_ghostty_surface_free_text ghostty_gtk_surface_free_text

#endif /* TASKERS_GHOSTTY_BRIDGE_H */
