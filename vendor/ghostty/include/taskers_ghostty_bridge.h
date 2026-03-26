#ifndef TASKERS_GHOSTTY_BRIDGE_H
#define TASKERS_GHOSTTY_BRIDGE_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct taskers_ghostty_host taskers_ghostty_host_t;

typedef struct {
  const char *const *command_argv;
  size_t command_argc;
  const char *const *env_entries;
  size_t env_count;
} taskers_ghostty_host_options_s;

typedef struct {
  const char *working_directory;
  const char *title;
  const char *const *env_entries;
  size_t env_count;
} taskers_ghostty_surface_options_s;

taskers_ghostty_host_t *taskers_ghostty_host_new(
    const taskers_ghostty_host_options_s *);
void taskers_ghostty_host_free(taskers_ghostty_host_t *);
int taskers_ghostty_host_tick(taskers_ghostty_host_t *);
void *taskers_ghostty_surface_new(
    taskers_ghostty_host_t *,
    const taskers_ghostty_surface_options_s *);
int taskers_ghostty_surface_grab_focus(void *);

#ifdef __cplusplus
}
#endif

#endif /* TASKERS_GHOSTTY_BRIDGE_H */
