#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
P0-07 · Runtime Estimation 정확도 스파이크

기준선 §12.3 / §13.4 / §32 P0-07.

핵심 질문: **50-step calibration 이 전체 실행시간을 얼마나 정확히 예측하는가?**

기준선 §12.3 은 Stage-2(calibrated) 의 상대오차를 σ/μ = 0.15 로 가정하고,
§13.4 의 chance-constrained selection 전체가 그 값 위에 서 있다.

  DoD: Stage-2 calibration 의 상대 오차 σ/μ <= 0.20
  실패 시: ADR-007 수정. deadline 을 best-effort 로 재정의하고 §13.4 전면 재설계

측정 방법
  워크로드별로
    warmup 10 step  ->  calibrate 50 step  ->  예측  ->  실제 400 step 완주
  예측 오차 = (실제 - 예측) / 예측
  각 워크로드 3회 반복해 분산을 본다.
"""
import argparse
import json
import statistics
import sys
import time


def _fmt(x, n=4):
    return round(float(x), n)


# ══════════════════════════════════════════════════════════════════
# 워크로드 — 서로 다른 특성을 갖도록 구성
# ══════════════════════════════════════════════════════════════════

def make_workloads(torch, device):
    nn = torch.nn

    class TinyTransformer(nn.Module):
        """attention 중심. 기준선 workload_class = TRAINING"""
        def __init__(self, d=256, heads=4, layers=4):
            super().__init__()
            layer = nn.TransformerEncoderLayer(
                d_model=d, nhead=heads, dim_feedforward=d * 4,
                batch_first=True, dropout=0.0)
            self.enc = nn.TransformerEncoder(layer, num_layers=layers)
            self.head = nn.Linear(d, d)

        def forward(self, x):
            return self.head(self.enc(x))

    class SmallCNN(nn.Module):
        """conv 중심. 메모리 접근 패턴이 transformer 와 다르다"""
        def __init__(self, ch=64):
            super().__init__()
            self.net = nn.Sequential(
                nn.Conv2d(3, ch, 3, padding=1), nn.ReLU(),
                nn.Conv2d(ch, ch, 3, padding=1), nn.ReLU(),
                nn.Conv2d(ch, ch * 2, 3, stride=2, padding=1), nn.ReLU(),
                nn.Conv2d(ch * 2, ch * 2, 3, padding=1), nn.ReLU(),
            )
            self.head = nn.Linear(ch * 2 * 16 * 16, 10)

        def forward(self, x):
            h = self.net(x)
            return self.head(h.flatten(1))

    def wl_transformer_train():
        m = TinyTransformer().to(device)
        opt = torch.optim.AdamW(m.parameters(), lr=1e-4)
        x = torch.randn(16, 64, 256, device=device)
        y = torch.randn(16, 64, 256, device=device)

        def step():
            opt.zero_grad(set_to_none=True)
            loss = ((m(x) - y) ** 2).mean()
            loss.backward()
            opt.step()
        return step

    def wl_cnn_train():
        m = SmallCNN().to(device)
        opt = torch.optim.SGD(m.parameters(), lr=1e-2, momentum=0.9)
        x = torch.randn(32, 3, 32, 32, device=device)
        y = torch.randint(0, 10, (32,), device=device)
        lossfn = torch.nn.CrossEntropyLoss()

        def step():
            opt.zero_grad(set_to_none=True)
            loss = lossfn(m(x), y)
            loss.backward()
            opt.step()
        return step

    def wl_inference():
        m = TinyTransformer(d=384, heads=6, layers=6).to(device).eval()
        x = torch.randn(32, 128, 384, device=device)

        def step():
            with torch.no_grad():
                m(x)
        return step

    def wl_matmul():
        """순수 GEMM. 가장 안정적일 것으로 기대되는 대조군"""
        a = torch.randn(2048, 2048, device=device)
        b = torch.randn(2048, 2048, device=device)

        def step():
            (a @ b).sum()
        return step

    def wl_memory_bound():
        """메모리 대역폭 바운드. compute 와 특성이 다르다"""
        a = torch.randn(4_000_000, device=device)
        b = torch.randn(4_000_000, device=device)

        def step():
            (a * b + a).sum()
        return step

    return [
        ("transformer_train", wl_transformer_train),
        ("cnn_train", wl_cnn_train),
        ("inference", wl_inference),
        ("matmul", wl_matmul),
        ("memory_bound", wl_memory_bound),
    ]


# ══════════════════════════════════════════════════════════════════
# 측정
# ══════════════════════════════════════════════════════════════════

def time_steps(torch, step, n, sync=True):
    """n step 실행하고 각 step 소요시간(초) 리스트를 반환."""
    times = []
    for _ in range(n):
        if sync:
            torch.cuda.synchronize()
        t0 = time.perf_counter()
        step()
        if sync:
            torch.cuda.synchronize()
        times.append(time.perf_counter() - t0)
    return times


def run_trial(torch, name, factory, warmup, calib_steps, total_steps):
    """한 번의 시행: calibration -> 예측 -> 실제 완주 -> 오차."""
    step = factory()

    # 1. warmup — CUDA 컨텍스트·커널 캐시·오토튜닝 안정화
    time_steps(torch, step, warmup)

    # 2. calibration (기준선 §12.3 Stage-2)
    calib = time_steps(torch, step, calib_steps)
    calib_median = statistics.median(calib)
    predicted_total = calib_median * total_steps

    # 3. 실제 완주
    t0 = time.perf_counter()
    actual_times = time_steps(torch, step, total_steps)
    actual_total = time.perf_counter() - t0

    rel_err = (actual_total - predicted_total) / predicted_total

    return {
        "workload": name,
        "calib_median_ms": _fmt(calib_median * 1000, 3),
        "calib_p90_ms": _fmt(sorted(calib)[int(len(calib) * 0.9)] * 1000, 3),
        "predicted_total_s": _fmt(predicted_total, 3),
        "actual_total_s": _fmt(actual_total, 3),
        "relative_error": _fmt(rel_err),
        "actual_median_ms": _fmt(statistics.median(actual_times) * 1000, 3),
        "actual_p99_ms": _fmt(sorted(actual_times)[int(len(actual_times) * 0.99)] * 1000, 3),
    }


def run_duration_scaling(torch, name, factory, warmup, calib_steps, horizons):
    """지속시간에 따라 예측 오차가 커지는가.

    이것이 P0-07 의 가장 중요한 후속 질문이다.
    실제 Job 은 수 시간 돌지만 calibration 은 50 step 이다.
    짧은 구간에서 정확하다는 사실이 긴 구간을 보장하지 않는다.
    (thermal throttling · 클럭 드리프트 · 메모리 단편화)
    """
    step = factory()
    time_steps(torch, step, warmup)

    calib = time_steps(torch, step, calib_steps)
    calib_median = statistics.median(calib)

    rows = []
    cumulative = 0
    t_start = time.perf_counter()
    prev_n = 0

    for n in horizons:
        need = n - prev_n
        seg = time_steps(torch, step, need)
        cumulative += sum(seg)
        prev_n = n

        predicted = calib_median * n
        actual = cumulative
        rel = (actual - predicted) / predicted
        # 이 구간만의 step time (드리프트 관측용)
        seg_median = statistics.median(seg)
        drift = (seg_median - calib_median) / calib_median

        rows.append({
            "workload": name,
            "horizon_steps": n,
            "predicted_s": _fmt(predicted, 3),
            "actual_s": _fmt(actual, 3),
            "relative_error": _fmt(rel),
            "segment_median_ms": _fmt(seg_median * 1000, 3),
            "step_time_drift": _fmt(drift),
            "elapsed_wall_s": _fmt(time.perf_counter() - t_start, 1),
        })
    return calib_median, rows


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--warmup", type=int, default=10)
    ap.add_argument("--calib", type=int, default=50, help="기준선 §12.3 Stage-2 = 50 step")
    ap.add_argument("--total", type=int, default=400)
    ap.add_argument("--trials", type=int, default=3)
    ap.add_argument("--scaling", action="store_true",
                    help="지속시간 스케일링 측정 (오래 걸린다)")
    args = ap.parse_args()

    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except Exception:  # noqa: BLE001
        pass

    import torch

    print("P0-07 · Runtime Estimation 정확도")
    print("torch %s / cuda %s" % (torch.__version__, torch.cuda.is_available()))
    if not torch.cuda.is_available():
        print("CUDA 없음 — 이 스파이크는 GPU 를 요구한다")
        return 1
    dev = torch.device("cuda")
    print("device: %s" % torch.cuda.get_device_name(0))
    print("설정: warmup=%d calib=%d total=%d trials=%d"
          % (args.warmup, args.calib, args.total, args.trials))
    print("=" * 70)
    print()

    workloads = make_workloads(torch, dev)

    # ── 지속시간 스케일링 모드 ────────────────────────────────────
    if args.scaling:
        horizons = [400, 2000, 10000, 40000]
        scaling_rows = []
        # 오래 걸리므로 대표 워크로드 2종만
        for name, factory in [w for w in workloads
                              if w[0] in ("transformer_train", "matmul")]:
            print("[%s] 지속시간 스케일링" % name)
            cm, rows = run_duration_scaling(torch, name, factory,
                                            args.warmup, args.calib, horizons)
            print("  calibration median = %.3f ms" % (cm * 1000))
            for r in rows:
                print("  %6d step  예측 %7.2fs | 실제 %7.2fs | 오차 %+5.1f%% "
                      "| step drift %+5.1f%% | 경과 %5.1fs"
                      % (r["horizon_steps"], r["predicted_s"], r["actual_s"],
                         r["relative_error"] * 100, r["step_time_drift"] * 100,
                         r["elapsed_wall_s"]))
            scaling_rows.extend(rows)
            torch.cuda.empty_cache()
            print()

        worst = max(abs(r["relative_error"]) for r in scaling_rows)
        worst_drift = max(abs(r["step_time_drift"]) for r in scaling_rows)
        print("=" * 70)
        print("스케일링 종합")
        print("  최대 |상대오차|      %.1f%%" % (worst * 100))
        print("  최대 |step 드리프트| %.1f%%" % (worst_drift * 100))
        print()
        print("JSON_BEGIN")
        print(json.dumps({
            "mode": "duration_scaling",
            "device": torch.cuda.get_device_name(0),
            "horizons": horizons,
            "rows": scaling_rows,
            "worst_abs_rel_error": _fmt(worst),
            "worst_abs_step_drift": _fmt(worst_drift),
        }, ensure_ascii=False, indent=2))
        print("JSON_END")
        return 0

    all_rows = []
    summary = []

    for name, factory in workloads:
        rows = []
        for t in range(args.trials):
            r = run_trial(torch, name, factory, args.warmup, args.calib, args.total)
            r["trial"] = t
            rows.append(r)
            all_rows.append(r)
            torch.cuda.empty_cache()

        errs = [abs(r["relative_error"]) for r in rows]
        signed = [r["relative_error"] for r in rows]
        mean_abs = statistics.mean(errs)
        # 시행 간 예측 편차 (재현성)
        pred = [r["predicted_total_s"] for r in rows]
        pred_cv = (statistics.pstdev(pred) / statistics.mean(pred)) if len(pred) > 1 else 0.0

        summary.append({
            "workload": name,
            "mean_abs_rel_error": _fmt(mean_abs),
            "max_abs_rel_error": _fmt(max(errs)),
            "signed_errors": [_fmt(s) for s in signed],
            "prediction_cv": _fmt(pred_cv),
        })

        print("[%s]" % name)
        for r in rows:
            print("  trial%d  calib %.3fms -> 예측 %.2fs | 실제 %.2fs | 오차 %+.1f%%"
                  % (r["trial"], r["calib_median_ms"], r["predicted_total_s"],
                     r["actual_total_s"], r["relative_error"] * 100))
        print("  평균 |오차| = %.1f%%   최대 = %.1f%%   예측 변동계수 = %.1f%%"
              % (mean_abs * 100, max(errs) * 100, pred_cv * 100))
        print()

    # ── 종합 판정 ────────────────────────────────────────────────
    all_abs = [abs(r["relative_error"]) for r in all_rows]
    overall_mean = statistics.mean(all_abs)
    overall_max = max(all_abs)
    # σ/μ — 기준선 §12.3 이 Stage-2 에 대해 0.15 로 가정한 값
    signed_all = [r["relative_error"] for r in all_rows]
    sigma = statistics.pstdev(signed_all)

    print("=" * 70)
    print("종합")
    print("  전체 시행 수            %d" % len(all_rows))
    print("  평균 |상대오차|         %.1f%%" % (overall_mean * 100))
    print("  최대 |상대오차|         %.1f%%" % (overall_max * 100))
    print("  상대오차 표준편차 σ     %.4f  (기준선 §12.3 Stage-2 가정 = 0.15)" % sigma)
    print()

    dod_pass = sigma <= 0.20
    print("  DoD (σ <= 0.20): %s" % ("PASS" if dod_pass else "FAIL"))
    if not dod_pass:
        print("  -> ADR-007 수정 필요. deadline 을 best-effort 로 재정의하고")
        print("     §13.4 chance-constrained selection 재설계")
    print()
    print("JSON_BEGIN")
    print(json.dumps({
        "device": torch.cuda.get_device_name(0),
        "torch": torch.__version__,
        "config": {"warmup": args.warmup, "calib": args.calib,
                   "total": args.total, "trials": args.trials},
        "trials": all_rows,
        "per_workload": summary,
        "overall": {
            "mean_abs_rel_error": _fmt(overall_mean),
            "max_abs_rel_error": _fmt(overall_max),
            "sigma": _fmt(sigma),
            "dod_sigma_le_020": dod_pass,
        },
    }, ensure_ascii=False, indent=2))
    print("JSON_END")
    return 0 if dod_pass else 1


if __name__ == "__main__":
    sys.exit(main())
