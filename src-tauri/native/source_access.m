#import <AppKit/AppKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#include <stdlib.h>
#include <string.h>

// Each returned URL is retained across the FFI boundary. Rust owns exactly one release.
static void *retainURL(NSURL *url) { return url ? (__bridge_retained void *)url : NULL; }
static char *jsonResult(id value) {
    NSData *data = [NSJSONSerialization dataWithJSONObject:value options:0 error:nil];
    return data ? strndup(data.bytes, data.length) : NULL;
}
static char *bookmark(NSURL *url) {
    NSError *error = nil;
    NSData *data = [url bookmarkDataWithOptions:(NSURLBookmarkCreationWithSecurityScope |
        NSURLBookmarkCreationSecurityScopeAllowOnlyReadAccess)
        includingResourceValuesForKeys:nil relativeToURL:nil error:&error];
    if (!data) return jsonResult(@{@"error":error.localizedDescription ?: @"Could not store permission. Choose the source again."});
    NSMutableArray *bytes = [NSMutableArray arrayWithCapacity:data.length];
    const unsigned char *raw = data.bytes;
    for (NSUInteger i = 0; i < data.length; i++) [bytes addObject:@(raw[i])];
    return jsonResult(@{@"path":url.path, @"bookmark":bytes});
}
// Panels and NSWorkspace are called on Tauri's main thread only.
char *chip_choose_source(bool directory) {
    @autoreleasepool {
        NSOpenPanel *panel = [NSOpenPanel openPanel];
        panel.canChooseDirectories = directory;
        panel.canChooseFiles = !directory;
        panel.allowsMultipleSelection = NO;
        panel.showsHiddenFiles = YES;
        panel.message = @"Choose Claude Code projects, Codex sessions or archived_sessions, or a JSONL log. Chip Count reads logs without changing them.";
        panel.prompt = @"Grant read access";
        if (!directory) panel.allowedContentTypes = @[[UTType typeWithFilenameExtension:@"jsonl"]];
        if ([panel runModal] != NSModalResponseOK) return jsonResult(@{@"cancelled":@YES});
        return bookmark(panel.URL);
    }
}
void *chip_resolve(const unsigned char *bytes, size_t count, bool *stale) {
    @autoreleasepool {
        BOOL isStale = NO;
        NSURL *url = [NSURL URLByResolvingBookmarkData:[NSData dataWithBytes:bytes length:count]
            options:(NSURLBookmarkResolutionWithSecurityScope | NSURLBookmarkResolutionWithoutUI)
            relativeToURL:nil bookmarkDataIsStale:&isStale error:nil];
        *stale = isStale;
        return retainURL(url);
    }
}
char *chip_refresh(void *handle) { @autoreleasepool { return bookmark((__bridge NSURL *)handle); } }
char *chip_url_path(void *handle) { @autoreleasepool { return strdup([((__bridge NSURL *)handle).path UTF8String]); } }
bool chip_start(void *handle) { return [(__bridge NSURL *)handle startAccessingSecurityScopedResource]; }
void chip_stop(void *handle) { [(__bridge NSURL *)handle stopAccessingSecurityScopedResource]; }
void chip_release(void *handle) { CFRelease(handle); }
void chip_free(char *value) { free(value); }
void *chip_save_panel(const char *filename) {
    @autoreleasepool {
        NSSavePanel *panel = [NSSavePanel savePanel];
        panel.nameFieldStringValue = [NSString stringWithUTF8String:filename];
        if ([panel runModal] != NSModalResponseOK) return NULL;
        return retainURL(panel.URL);
    }
}
void chip_reveal(const char *path) {
    @autoreleasepool {
        [[NSWorkspace sharedWorkspace] activateFileViewerSelectingURLs:@[[NSURL fileURLWithPath:[NSString stringWithUTF8String:path]]]];
    }
}
