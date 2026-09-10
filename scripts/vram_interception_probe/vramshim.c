// VRAM 상한을 유저스페이스에서 거는 최소 shim (HAMi-core 의 원리만 뽑음)
//   빌드:  gcc -shared -fPIC -o vramshim.so vramshim.c -ldl -lpthread
//   사용:  LD_PRELOAD=./vramshim.so GPUTEER_VRAM_LIMIT_MIB=2048 ./프로그램
// ★ 이건 실험이다. 프로덕션 코드가 아니다.
//
// ══════════════════════════════════════════════════════════════════
// ★★ 2026-09-08 — 1차 판이 틀렸다. 독립 검수가 잡았다
// ══════════════════════════════════════════════════════════════════
// 1차 판의 `cuMemFree_v2` 는 진짜 함수로 **전달만 하고 `used` 를 줄이지
// 않았다.** 그리고 같이 쓴 `alloc_probe.c` 는 free 를 **한 번도 부르지
// 않았다.** 그래서 1차 실측이 잰 것은 "VRAM **사용량** 상한" 이 아니라
// **"프로세스 생애 누적 할당 상한"** 이었다.
//
//   무슨 차이인가  512MiB 를 잡았다 풀었다 5번 반복하는 정상 작업은
//                  실제로 512MiB 만 쓴다. 그런데 1차 판은 상한 2048 에서
//                  그 작업을 거부한다. **막는 게 아니라 죽인다.**
//
// ★ 대조군을 둘이나 뒀는데 못 잡았다. 이유가 분명하다 — **대조군 셋이
//   전부 free 를 안 불렀다.** 대조군은 "결과가 다른가" 를 봐야 하는데,
//   같은 경로만 세 번 밟으면 그 경로 밖의 결함은 보이지 않는다.
//   그래서 이번 판은 `alloc_free_loop` 대조군을 추가한다.
//
// 2차 판이 고친 것:
//   1. free 가 `used` 를 실제로 줄인다 (포인터별 크기를 기억한다)
//   2. 검사와 할당을 **한 락 안에서** 한다 — 1차 판은 검사 후 락을 풀고
//      할당해서, 여러 스레드가 동시에 통과하면 상한을 넘을 수 있었다
//   3. 최대 동시 사용량(peak)을 따로 보고한다
// ══════════════════════════════════════════════════════════════════
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <dlfcn.h>
#include <pthread.h>

typedef int CUresult;                    // CUDA_SUCCESS=0, OUT_OF_MEMORY=2
typedef unsigned long long CUdeviceptr;

static CUresult (*real_alloc)(CUdeviceptr*, size_t) = NULL;
static CUresult (*real_free)(CUdeviceptr) = NULL;

// 포인터별 크기. free 가 얼마를 돌려줘야 하는지 알아야 한다.
// ★ 실험용이라 선형 탐색이다. 프로덕션이라면 해시가 필요하다.
#define MAX_LIVE 4096
static struct { CUdeviceptr p; size_t n; } live[MAX_LIVE];
static int live_n = 0;

static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static size_t used = 0, peak = 0, limit = 0;
static int inited = 0;

static void init_locked(void) {
    if (inited) return;
    inited = 1;
    const char *e = getenv("GPUTEER_VRAM_LIMIT_MIB");
    limit = e ? (size_t)atoll(e) * 1024ULL * 1024ULL : 0;
    fprintf(stderr, "[shim] 켜짐. 상한 = %s\n",
            limit ? e : "(없음 — 통과만 시킨다)");
}

CUresult cuMemAlloc_v2(CUdeviceptr *dptr, size_t bytes) {
    if (!real_alloc) real_alloc = dlsym(RTLD_NEXT, "cuMemAlloc_v2");
    if (!real_alloc) { fprintf(stderr, "[shim] ★ 진짜 함수를 못 찾았다\n"); return 1; }

    // ★ 검사부터 장부 기록까지 **락을 놓지 않는다.** 1차 판은 여기서
    //   락을 풀고 할당해서 여러 스레드가 동시에 통과할 수 있었다.
    pthread_mutex_lock(&lock);
    init_locked();

    if (limit && used + bytes > limit) {
        fprintf(stderr, "[shim] 거부: %zuMiB 요청, 지금 %zuMiB 쓰는 중, 상한 %zuMiB\n",
                bytes>>20, used>>20, limit>>20);
        pthread_mutex_unlock(&lock);
        return 2;                        // CUDA_ERROR_OUT_OF_MEMORY
    }

    CUresult rc = real_alloc(dptr, bytes);
    if (rc == 0) {
        if (live_n < MAX_LIVE) { live[live_n].p = *dptr; live[live_n].n = bytes; live_n++; }
        else fprintf(stderr, "[shim] ★ 장부가 넘쳤다 — 이 실험의 한계다\n");
        used += bytes;
        if (used > peak) peak = used;
        fprintf(stderr, "[shim] 허용: %zuMiB (지금 %zuMiB, 최대 %zuMiB)\n",
                bytes>>20, used>>20, peak>>20);
    }
    pthread_mutex_unlock(&lock);
    return rc;
}

CUresult cuMemFree_v2(CUdeviceptr p) {
    if (!real_free) real_free = dlsym(RTLD_NEXT, "cuMemFree_v2");
    if (!real_free) return 1;

    pthread_mutex_lock(&lock);
    init_locked();
    // ★ 실제로 줄인다. 1차 판이 빠뜨린 것이 정확히 이 세 줄이다.
    for (int i = 0; i < live_n; i++) {
        if (live[i].p == p) {
            used -= live[i].n;
            fprintf(stderr, "[shim] 해제: %zuMiB (지금 %zuMiB, 최대 %zuMiB)\n",
                    live[i].n>>20, used>>20, peak>>20);
            live[i] = live[--live_n];
            break;
        }
    }
    pthread_mutex_unlock(&lock);
    return real_free(p);
}
