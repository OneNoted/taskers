#ifndef GHOSTTY_GTK_H
#define GHOSTTY_GTK_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct ghostty_gtk_host ghostty_gtk_host_t;

typedef struct {
  const char *const *command_argv;
  size_t command_argc;
  const char *const *env_entries;
  size_t env_count;
  const char *base_config_path;
  const char *override_config_path;
} ghostty_gtk_host_options_s;

typedef struct {
  const char *working_directory;
  const char *title;
  const char *const *env_entries;
  size_t env_count;
} ghostty_gtk_surface_options_s;

typedef struct {
  const char *text;
  size_t text_len;
} ghostty_gtk_text_s;

/*
 * ghostty_gtk_surface_new returns a GtkWidget*.
 *
 * GTK hosts can observe these properties via notify signals on the returned
 * widget to react to title/cwd/process lifecycle changes without depending on
 * Taskers-specific bridge naming.
 */
#define GHOSTTY_GTK_PROPERTY_TITLE "title"
#define GHOSTTY_GTK_PROPERTY_PWD "pwd"
#define GHOSTTY_GTK_PROPERTY_CHILD_EXITED "child-exited"

ghostty_gtk_host_t *ghostty_gtk_host_new(const ghostty_gtk_host_options_s *);
void ghostty_gtk_host_free(ghostty_gtk_host_t *);
const char *ghostty_gtk_host_version(void);
const char *ghostty_gtk_host_build_id(void);
void ghostty_gtk_host_begin_shutdown(ghostty_gtk_host_t *);
size_t ghostty_gtk_host_surface_count(ghostty_gtk_host_t *);
int ghostty_gtk_host_tick(ghostty_gtk_host_t *);
void *ghostty_gtk_surface_new(ghostty_gtk_host_t *,
                              const ghostty_gtk_surface_options_s *);
void ghostty_gtk_surface_destroy(void *);
int ghostty_gtk_surface_grab_focus(void *);
int ghostty_gtk_surface_has_selection(void *);
int ghostty_gtk_surface_send_text(void *, const char *, size_t);
int ghostty_gtk_surface_read_all_text(void *, ghostty_gtk_text_s *);
void ghostty_gtk_surface_free_text(ghostty_gtk_text_s *);

#ifdef __cplusplus
}
#endif

#endif /* GHOSTTY_GTK_H */
