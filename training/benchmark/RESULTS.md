# Speech-to-text benchmark: public set (Svarah)

Indian-accented English from AI4Bharat's Svarah test set (CC BY 4.0). Scores are numbers only; the private set (my own voice) is scored in `private/RESULTS.md`, which is git-ignored.

WER = word error rate in %, after lower-casing, dropping punctuation and writing numbers as digits. Lower is better. Ready to send = output equals the reference exactly, capitals and punctuation included (a final period may differ). Svarah's references are plain transcripts, not text a person would send, so **ready to send is not meaningful on this set**: read WER.

Seconds per clip = the whole command for one clip, so for Lipik, Handy and Qwen3-ASR it includes loading the model each time (the app keeps it loaded) and, for Lipik, the clean-up. Model time = speech-to-text only. Peak memory = the largest the benchmark process got, model worker included; Apple's model runs inside a macOS system service, so its number leaves the model out.

### All clips (150)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |   150 |   8.5 |              20 |                                 0.46 |                  0.06 |            934 |
| Lipik, raw Parakeet text                |   150 |   8.5 |              23 |                                 0.46 |                  0.06 |            934 |
| Apple Dictation en-IN                   |   150 |  18.3 |               5 |                                 0.15 |                  0.15 |             22 |
| Qwen3-ASR 1.7B, raw                     |   150 |  12.7 |              13 |                                 1.23 |                  0.24 |           3690 |
| Whisper large-v3-turbo, raw             |   150 |   7.5 |              30 |                                 1.77 |                  0.69 |           1173 |

### assamese (4 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |     4 |   8.0 |              25 |                                 0.45 |                  0.05 |            921 |
| Lipik, raw Parakeet text                |     4 |   8.0 |              25 |                                 0.45 |                  0.05 |            921 |
| Apple Dictation en-IN                   |     4 |  44.0 |               0 |                                 0.13 |                  0.13 |             22 |
| Qwen3-ASR 1.7B, raw                     |     4 |  52.0 |               0 |                                 1.24 |                  0.18 |           3676 |
| Whisper large-v3-turbo, raw             |     4 |   8.0 |              25 |                                 1.68 |                  0.63 |           1170 |

### bengali (10 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |    10 |   4.2 |              10 |                                 0.46 |                  0.07 |            932 |
| Lipik, raw Parakeet text                |    10 |   4.2 |              10 |                                 0.46 |                  0.07 |            932 |
| Apple Dictation en-IN                   |    10 |  12.5 |              10 |                                 0.14 |                  0.14 |             22 |
| Qwen3-ASR 1.7B, raw                     |    10 |   3.6 |              10 |                                 1.21 |                  0.30 |           3679 |
| Whisper large-v3-turbo, raw             |    10 |   1.2 |              50 |                                 1.78 |                  0.72 |           1170 |

### bodo (10 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |    10 |   7.9 |              20 |                                 0.45 |                  0.06 |            925 |
| Lipik, raw Parakeet text                |    10 |   7.9 |              20 |                                 0.45 |                  0.06 |            925 |
| Apple Dictation en-IN                   |    10 |  30.2 |              10 |                                 0.18 |                  0.18 |             22 |
| Qwen3-ASR 1.7B, raw                     |    10 |   7.9 |              10 |                                 1.14 |                  0.21 |           3688 |
| Whisper large-v3-turbo, raw             |    10 |   7.9 |              20 |                                 1.79 |                  0.67 |           1171 |

### dogri (9 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |     9 |  10.4 |              33 |                                 0.46 |                  0.06 |            921 |
| Lipik, raw Parakeet text                |     9 |  10.4 |              33 |                                 0.46 |                  0.06 |            921 |
| Apple Dictation en-IN                   |     9 |  17.7 |              22 |                                 0.11 |                  0.11 |             22 |
| Qwen3-ASR 1.7B, raw                     |     9 |   6.2 |              22 |                                 1.25 |                  0.25 |           3681 |
| Whisper large-v3-turbo, raw             |     9 |  13.5 |              22 |                                 1.78 |                  0.71 |           1173 |

### gujarati (5 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |     5 |   3.1 |               0 |                                 0.45 |                  0.06 |            927 |
| Lipik, raw Parakeet text                |     5 |   3.1 |              20 |                                 0.45 |                  0.06 |            927 |
| Apple Dictation en-IN                   |     5 |   6.2 |               0 |                                 0.12 |                  0.12 |             22 |
| Qwen3-ASR 1.7B, raw                     |     5 |   1.6 |               0 |                                 1.17 |                  0.20 |           3683 |
| Whisper large-v3-turbo, raw             |     5 |   3.1 |              20 |                                 1.85 |                  0.73 |           1169 |

### hindi (7 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |     7 |   5.8 |               0 |                                 0.46 |                  0.06 |            924 |
| Lipik, raw Parakeet text                |     7 |   5.8 |               0 |                                 0.46 |                  0.06 |            924 |
| Apple Dictation en-IN                   |     7 |  10.5 |               0 |                                 0.13 |                  0.13 |             22 |
| Qwen3-ASR 1.7B, raw                     |     7 |   7.0 |               0 |                                 1.18 |                  0.22 |           3680 |
| Whisper large-v3-turbo, raw             |     7 |   4.7 |              29 |                                 1.70 |                  0.65 |           1168 |

### kannada (11 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |    11 |   5.9 |               9 |                                 0.46 |                  0.06 |            934 |
| Lipik, raw Parakeet text                |    11 |   5.9 |               9 |                                 0.46 |                  0.06 |            934 |
| Apple Dictation en-IN                   |    11 |  12.2 |               0 |                                 0.19 |                  0.19 |             22 |
| Qwen3-ASR 1.7B, raw                     |    11 |  10.4 |               0 |                                 1.37 |                  0.34 |           3690 |
| Whisper large-v3-turbo, raw             |    11 |   5.4 |               9 |                                 1.71 |                  0.68 |           1171 |

### kashmiri (8 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |     8 |  10.6 |              25 |                                 0.46 |                  0.06 |            931 |
| Lipik, raw Parakeet text                |     8 |  10.6 |              25 |                                 0.46 |                  0.06 |            931 |
| Apple Dictation en-IN                   |     8 |  25.2 |               0 |                                 0.23 |                  0.23 |             22 |
| Qwen3-ASR 1.7B, raw                     |     8 |  17.9 |              12 |                                 1.16 |                  0.23 |           3687 |
| Whisper large-v3-turbo, raw             |     8 |  11.4 |               0 |                                 1.81 |                  0.73 |           1171 |

### konkani (8 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |     8 |  12.8 |              12 |                                 0.46 |                  0.07 |            920 |
| Lipik, raw Parakeet text                |     8 |  12.8 |              12 |                                 0.46 |                  0.07 |            920 |
| Apple Dictation en-IN                   |     8 |  14.0 |              12 |                                 0.16 |                  0.16 |             22 |
| Qwen3-ASR 1.7B, raw                     |     8 |  29.1 |              12 |                                 1.23 |                  0.31 |           3681 |
| Whisper large-v3-turbo, raw             |     8 |  10.5 |              38 |                                 1.85 |                  0.74 |           1170 |

### maithili (7 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |     7 |   9.1 |              14 |                                 0.45 |                  0.06 |            922 |
| Lipik, raw Parakeet text                |     7 |   9.1 |              14 |                                 0.45 |                  0.06 |            922 |
| Apple Dictation en-IN                   |     7 |  11.7 |              14 |                                 0.16 |                  0.16 |             22 |
| Qwen3-ASR 1.7B, raw                     |     7 |   2.6 |              29 |                                 1.21 |                  0.22 |           3680 |
| Whisper large-v3-turbo, raw             |     7 |   6.5 |              14 |                                 1.67 |                  0.64 |           1173 |

### malayalam (9 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |     9 |  15.3 |              22 |                                 0.46 |                  0.06 |            920 |
| Lipik, raw Parakeet text                |     9 |  15.3 |              22 |                                 0.46 |                  0.06 |            920 |
| Apple Dictation en-IN                   |     9 |  15.3 |              11 |                                 0.12 |                  0.12 |             22 |
| Qwen3-ASR 1.7B, raw                     |     9 |  10.6 |              11 |                                 1.17 |                  0.22 |           3679 |
| Whisper large-v3-turbo, raw             |     9 |  16.5 |              33 |                                 1.78 |                  0.71 |           1168 |

### marathi (7 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |     7 |   8.5 |               0 |                                 0.44 |                  0.05 |            921 |
| Lipik, raw Parakeet text                |     7 |   8.5 |               0 |                                 0.44 |                  0.05 |            921 |
| Apple Dictation en-IN                   |     7 |  19.7 |               0 |                                 0.16 |                  0.16 |             22 |
| Qwen3-ASR 1.7B, raw                     |     7 |   8.5 |               0 |                                 1.14 |                  0.21 |           3678 |
| Whisper large-v3-turbo, raw             |     7 |   8.5 |               0 |                                 1.81 |                  0.73 |           1168 |

### nepali (13 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |    13 |   6.3 |              23 |                                 0.48 |                  0.08 |            926 |
| Lipik, raw Parakeet text                |    13 |   6.3 |              23 |                                 0.48 |                  0.08 |            926 |
| Apple Dictation en-IN                   |    13 |  20.6 |               0 |                                 0.20 |                  0.20 |             22 |
| Qwen3-ASR 1.7B, raw                     |    13 |   5.7 |              15 |                                 1.28 |                  0.34 |           3683 |
| Whisper large-v3-turbo, raw             |    13 |   3.4 |              38 |                                 1.84 |                  0.73 |           1172 |

### odia (6 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |     6 |   3.9 |              33 |                                 0.46 |                  0.07 |            922 |
| Lipik, raw Parakeet text                |     6 |   3.9 |              50 |                                 0.46 |                  0.07 |            922 |
| Apple Dictation en-IN                   |     6 |  17.1 |               0 |                                 0.14 |                  0.14 |             22 |
| Qwen3-ASR 1.7B, raw                     |     6 |   2.6 |              17 |                                 1.27 |                  0.28 |           3679 |
| Whisper large-v3-turbo, raw             |     6 |   2.6 |              50 |                                 1.67 |                  0.63 |           1167 |

### punjabi (5 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |     5 |   1.7 |              60 |                                 0.45 |                  0.06 |            918 |
| Lipik, raw Parakeet text                |     5 |   1.7 |              60 |                                 0.45 |                  0.06 |            918 |
| Apple Dictation en-IN                   |     5 |  24.1 |               0 |                                 0.14 |                  0.14 |             22 |
| Qwen3-ASR 1.7B, raw                     |     5 |   3.4 |              60 |                                 1.27 |                  0.23 |           3679 |
| Whisper large-v3-turbo, raw             |     5 |   5.2 |              40 |                                 1.74 |                  0.66 |           1168 |

### sindhi (5 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |     5 |  12.0 |              20 |                                 0.45 |                  0.07 |            923 |
| Lipik, raw Parakeet text                |     5 |  12.0 |              40 |                                 0.45 |                  0.07 |            923 |
| Apple Dictation en-IN                   |     5 |   6.0 |               0 |                                 0.13 |                  0.13 |             22 |
| Qwen3-ASR 1.7B, raw                     |     5 |  54.0 |              20 |                                 1.52 |                  0.22 |           3676 |
| Whisper large-v3-turbo, raw             |     5 |   6.0 |              60 |                                 1.76 |                  0.70 |           1172 |

### tamil (8 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |     8 |   5.6 |              38 |                                 0.45 |                  0.05 |            922 |
| Lipik, raw Parakeet text                |     8 |   5.6 |              50 |                                 0.45 |                  0.05 |            922 |
| Apple Dictation en-IN                   |     8 |  15.5 |              12 |                                 0.12 |                  0.12 |             22 |
| Qwen3-ASR 1.7B, raw                     |     8 |  11.3 |              12 |                                 1.24 |                  0.19 |           3681 |
| Whisper large-v3-turbo, raw             |     8 |   1.4 |              88 |                                 1.83 |                  0.70 |           1169 |

### telugu (6 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |     6 |   5.5 |              17 |                                 0.45 |                  0.06 |            922 |
| Lipik, raw Parakeet text                |     6 |   5.5 |              17 |                                 0.45 |                  0.06 |            922 |
| Apple Dictation en-IN                   |     6 |  19.2 |               0 |                                 0.18 |                  0.18 |             22 |
| Qwen3-ASR 1.7B, raw                     |     6 |  30.1 |              17 |                                 1.48 |                  0.24 |           3682 |
| Whisper large-v3-turbo, raw             |     6 |   8.2 |              17 |                                 1.78 |                  0.70 |           1170 |

### urdu (12 clips)

| Pipeline                                | Clips | WER % | Ready to send % | Seconds per clip (median, whole run) | Model time (median s) | Peak memory MB |
| --------------------------------------- | ----: | ----: | --------------: | -----------------------------------: | --------------------: | -------------: |
| Lipik, final text (Parakeet + clean-up) |    12 |  24.0 |              25 |                                 0.46 |                  0.06 |            926 |
| Lipik, raw Parakeet text                |    12 |  24.0 |              25 |                                 0.46 |                  0.06 |            926 |
| Apple Dictation en-IN                   |    12 |  34.1 |               0 |                                 0.17 |                  0.17 |             22 |
| Qwen3-ASR 1.7B, raw                     |    12 |  27.9 |              17 |                                 1.23 |                  0.25 |           3684 |
| Whisper large-v3-turbo, raw             |    12 |  19.4 |              25 |                                 1.75 |                  0.68 |           1170 |
