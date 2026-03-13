#ifndef TASKERS_GHOSTTY_BRIDGE_H
#define TASKERS_GHOSTTY_BRIDGE_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct taskers_ghostty_host taskers_ghostty_host_t;

typedef struct {
  const char *working_directory;
  const char *title;
} taskers_ghostty_surface_options_s;

taskers_ghostty_host_t *taskers_ghostty_host_new(void);
void taskers_ghostty_host_free(taskers_ghostty_host_t *);
int taskers_ghostty_host_tick(taskers_ghostty_host_t *);
void *taskers_ghostty_surface_new(
    taskers_ghostty_host_t *,
    const taskers_ghostty_surface_options_s *);

#ifdef __cplusplus
}
#endif

#endif /* TASKERS_GHOSTTY_BRIDGE_H */
