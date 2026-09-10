// 512MiB 단위로 잡아 본다. 어디서 막히는지 본다.
//   ★ 평범한 CUDA 앱과 같은 방식으로 링크한다(-lcuda) — 그래야
//     LD_PRELOAD 가로채기가 실제 앱과 같은 경로를 탄다.
//
// ★★ 2026-09-08 2차 — 독립 검수가 1차의 결함을 잡았다.
//   1차 판은 `cuMemFree` 를 **한 번도 부르지 않았다.** 그래서 상한이
//   "동시 사용량 상한" 인지 "생애 누적 상한" 인지 구분하지 못했다.
//   대조군을 셋이나 뒀는데도 못 잡았다 — **셋 다 같은 경로만 밟았기
//   때문이다.** 대조군은 "결과가 다른가" 를 봐야 한다.
//   그래서 `loop` 모드를 넣는다: 잡고 풀기를 반복한다. 동시 사용량은
//   늘 512MiB 다. 이게 거부당하면 그건 상한이 아니라 고장이다.
//
//   쓰기:  alloc_probe hold <개수>   잡기만 한다 (안 푼다)
//          alloc_probe loop <횟수>   잡았다 푼다를 반복한다
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef int CUresult;
typedef unsigned long long CUdeviceptr;
typedef int CUdevice;
typedef void* CUcontext;
extern CUresult cuInit(unsigned int);
extern CUresult cuDeviceGet(CUdevice*, int);
extern CUresult cuCtxCreate_v2(CUcontext*, unsigned int, CUdevice);
extern CUresult cuMemAlloc_v2(CUdeviceptr*, size_t);
extern CUresult cuMemFree_v2(CUdeviceptr);
extern CUresult cuMemGetInfo_v2(size_t*, size_t*);

int main(int argc, char **argv) {
    const char *mode = argc > 1 ? argv[1] : "hold";
    int n = argc > 2 ? atoi(argv[2]) : 8;
    size_t CH = 512ULL << 20;
    CUresult rc;
    if ((rc = cuInit(0)))            { printf("RESULT stage=cuInit rc=%d\n", rc); return 1; }
    CUdevice d;
    if ((rc = cuDeviceGet(&d, 0)))   { printf("RESULT stage=cuDeviceGet rc=%d\n", rc); return 1; }
    CUcontext c;
    if ((rc = cuCtxCreate_v2(&c,0,d))){ printf("RESULT stage=cuCtxCreate rc=%d\n", rc); return 1; }
    size_t fr, tot; cuMemGetInfo_v2(&fr, &tot);
    printf("RESULT stage=ready mode=%s free_mib=%zu total_mib=%zu\n", mode, fr>>20, tot>>20);

    if (!strcmp(mode, "loop")) {
        // ★ 동시 사용량은 늘 512MiB 다. 누적은 n x 512MiB 다.
        //   상한이 "동시" 면 통과해야 하고, "누적" 이면 막힌다.
        for (int i = 0; i < n; i++) {
            CUdeviceptr p;
            rc = cuMemAlloc_v2(&p, CH);
            if (rc) { printf("RESULT stage=stopped mode=loop iteration=%d cumulative_mib=%zu rc=%d\n",
                             i, (size_t)i * (CH>>20), rc); return 0; }
            cuMemFree_v2(p);
        }
        printf("RESULT stage=all_ok mode=loop iterations=%d cumulative_mib=%zu peak_concurrent_mib=%zu\n",
               n, (size_t)n * (CH>>20), CH>>20);
        return 0;
    }

    size_t got = 0;
    for (int i = 0; i < n; i++) {
        CUdeviceptr p;
        rc = cuMemAlloc_v2(&p, CH);
        if (rc) { printf("RESULT stage=stopped mode=hold at_mib=%zu next_mib=%zu rc=%d\n", got, CH>>20, rc); return 0; }
        got += CH >> 20;
    }
    printf("RESULT stage=all_ok mode=hold allocated_mib=%zu\n", got);
    return 0;
}
