#ifndef TASKERS_MACOS_FFI_H
#define TASKERS_MACOS_FFI_H

#include <stdbool.h>
#include <stdint.h>

typedef struct taskers_macos_core taskers_macos_core_t;

taskers_macos_core_t *taskers_macos_core_new_with_options_json(
    const char *options_json
);
taskers_macos_core_t *taskers_macos_core_new(
    const char *session_path,
    const char *socket_path,
    const char *configured_shell,
    bool demo
);
void taskers_macos_core_free(taskers_macos_core_t *core);

char *taskers_macos_core_snapshot_json(const taskers_macos_core_t *core);
char *taskers_macos_core_dispatch_json(taskers_macos_core_t *core, const char *command_json);
char *taskers_macos_core_surface_descriptor_json(
    const taskers_macos_core_t *core,
    const char *workspace_id,
    const char *pane_id
);

uint64_t taskers_macos_core_revision(const taskers_macos_core_t *core);
char *taskers_macos_last_error_message(void);
void taskers_macos_string_free(char *value);

#endif
