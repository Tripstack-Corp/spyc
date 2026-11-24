#include <stdio.h>
#include <stdlib.h>
#include <sys/sysctl.h>
int main(int argc, char **argv) {
  for (int i = 1; i < argc; i++) {
    int pid = atoi(argv[i]);
    int mib[4] = {CTL_KERN, KERN_PROC, KERN_PROC_PID, pid};
    struct kinfo_proc kp; size_t len = sizeof kp;
    if (sysctl(mib, 4, &kp, &len, NULL, 0) != 0 || len == 0) { printf("%d: gone\n", pid); continue; }
    printf("%d: pid=%d sigignore=0x%08x sigcatch=0x%08x  (SIGPIPE ignored=%d caught=%d; SIGTTOU ignored=%d; SIGHUP ignored=%d)\n", pid, kp.kp_proc.p_pid,
      kp.kp_proc.p_sigignore, kp.kp_proc.p_sigcatch,
      !!(kp.kp_proc.p_sigignore & (1u<<(13-1))), !!(kp.kp_proc.p_sigcatch & (1u<<(13-1))),
      !!(kp.kp_proc.p_sigignore & (1u<<(22-1))), !!(kp.kp_proc.p_sigignore & (1u<<(1-1))));
  }
  return 0;
}
