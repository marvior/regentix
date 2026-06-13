python -m mlx_lm.lora \
    --model Qwen/Qwen2.5-Coder-1.5B-Instruct  \
    --data ./data \
    --adapter-path /Users/waltermolino/rustProjects/mcp-proxy/model_ai/rego_finetuning/adapters \
    --iters 400 \
    --steps-per-eval 40  \
    --batch-size 2 \
    --learning-rate 5e-6 \
    --train
