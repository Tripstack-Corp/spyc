__attribute__((weak)) unsigned weak_default(unsigned x) { return x * 3; }
__attribute__((weak, visibility("hidden"))) unsigned weak_hidden(unsigned x) { return x * 5; }
__attribute__((visibility("hidden"))) unsigned strong_hidden(unsigned x) { return x * 7; }
