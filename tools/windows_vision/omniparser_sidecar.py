#!/usr/bin/env python3
"""Local OmniParser region detector for the Windows screenshot fallback.

The service intentionally returns only screen coordinates. DeviceKit keeps
clicking through its existing native input API, so screenshots never need to
leave the developer machine and the detector never receives control authority.
"""

from __future__ import annotations

import argparse
import base64
import io
import json
import os
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

import numpy as np
import torch
from huggingface_hub import hf_hub_download
from PIL import Image

MODEL_REPOSITORY = "microsoft/OmniParser-v2.0"
MODEL_FILENAME = "icon_detect_v3/model.pt"
IMAGE_SIZE = 1280
STRIDES = (8, 16, 32)


class OmniParserDetector:
    def __init__(self, threshold: float, iou_threshold: float) -> None:
        model_path = os.environ.get("DEVICE_KIT_OMNIPARSER_MODEL")
        if not model_path:
            model_path = hf_hub_download(
                repo_id=MODEL_REPOSITORY,
                filename=MODEL_FILENAME,
            )
        self._model = torch.jit.load(model_path, map_location="cpu").eval()
        self._threshold = threshold
        self._iou_threshold = iou_threshold

    def locate(self, png: bytes) -> list[dict[str, Any]]:
        image = Image.open(io.BytesIO(png)).convert("RGB")
        width, height = image.size
        if width <= 0 or height <= 0:
            raise ValueError("screenshot has invalid dimensions")

        scale = min(IMAGE_SIZE / width, IMAGE_SIZE / height)
        resized = image.resize(
            (round(width * scale), round(height * scale)), Image.Resampling.BILINEAR
        )
        canvas = Image.new("RGB", (IMAGE_SIZE, IMAGE_SIZE), (114, 114, 114))
        canvas.paste(resized, (0, 0))
        tensor = torch.from_numpy(np.array(canvas)).permute(2, 0, 1).float()[None] / 255.0

        with torch.no_grad():
            outputs = self._model(tensor)
        boxes, scores = self._decode(outputs)
        if boxes.numel() == 0:
            return []
        boxes = boxes / scale
        boxes[:, 0::2] = boxes[:, 0::2].clamp(0, width)
        boxes[:, 1::2] = boxes[:, 1::2].clamp(0, height)

        elements: list[dict[str, Any]] = []
        for index, (box, score) in enumerate(zip(boxes.tolist(), scores.tolist())):
            x1, y1, x2, y2 = box
            elements.append(
                {
                    "id": f"region-{index}",
                    "label": f"interactable region {index}",
                    "score": round(float(score), 4),
                    "bounds": {
                        "x": round(x1, 2),
                        "y": round(y1, 2),
                        "width": round(x2 - x1, 2),
                        "height": round(y2 - y1, 2),
                    },
                }
            )
        return elements

    def _decode(self, outputs: Any) -> tuple[torch.Tensor, torch.Tensor]:
        boxes: list[torch.Tensor] = []
        scores: list[torch.Tensor] = []
        for index, stride in enumerate(STRIDES):
            logits = outputs[index * 2].sigmoid()[0, 0]
            ltrb = outputs[index * 2 + 1][0]
            grid = logits.shape[-1]
            gy, gx = torch.meshgrid(
                torch.arange(grid, dtype=torch.float32),
                torch.arange(grid, dtype=torch.float32),
                indexing="ij",
            )
            center_x, center_y = gx + 0.5, gy + 0.5
            left, top, right, bottom = ltrb
            boxes.append(
                torch.stack(
                    (
                        (center_x - left) * stride,
                        (center_y - top) * stride,
                        (center_x + right) * stride,
                        (center_y + bottom) * stride,
                    ),
                    dim=-1,
                ).reshape(-1, 4)
            )
            scores.append(logits.reshape(-1))

        all_boxes = torch.cat(boxes)
        all_scores = torch.cat(scores)
        keep = all_scores > self._threshold
        all_boxes, all_scores = all_boxes[keep], all_scores[keep]
        if all_boxes.numel() == 0:
            return all_boxes, all_scores
        keep = _nms(all_boxes, all_scores, self._iou_threshold)
        return all_boxes[keep], all_scores[keep]


def _nms(boxes: torch.Tensor, scores: torch.Tensor, threshold: float) -> torch.Tensor:
    order = scores.argsort(descending=True)
    kept: list[torch.Tensor] = []
    while order.numel() > 0:
        current = order[0]
        kept.append(current)
        if order.numel() == 1:
            break
        remaining = order[1:]
        current_box = boxes[current]
        other_boxes = boxes[remaining]
        top_left = torch.maximum(current_box[:2], other_boxes[:, :2])
        bottom_right = torch.minimum(current_box[2:], other_boxes[:, 2:])
        intersection = (bottom_right - top_left).clamp(min=0).prod(dim=1)
        current_area = (current_box[2] - current_box[0]) * (current_box[3] - current_box[1])
        other_areas = (other_boxes[:, 2] - other_boxes[:, 0]) * (
            other_boxes[:, 3] - other_boxes[:, 1]
        )
        iou = intersection / (current_area + other_areas - intersection).clamp(min=1e-6)
        order = remaining[iou <= threshold]
    return torch.stack(kept)


class Handler(BaseHTTPRequestHandler):
    detector: OmniParserDetector

    def do_POST(self) -> None:  # noqa: N802
        if self.path != "/locate":
            self._send(HTTPStatus.NOT_FOUND, {"error": "use POST /locate"})
            return
        length = int(self.headers.get("Content-Length", "0"))
        if length <= 0 or length > 25 * 1024 * 1024:
            self._send(HTTPStatus.BAD_REQUEST, {"error": "invalid request size"})
            return
        try:
            request = json.loads(self.rfile.read(length))
            encoded = request["imageBase64"]
            image = base64.b64decode(encoded, validate=True)
            elements = self.detector.locate(image)
        except (KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
            self._send(HTTPStatus.BAD_REQUEST, {"error": str(error)})
            return
        except Exception as error:  # Do not kill the local service after one bad frame.
            self._send(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": str(error)})
            return
        self._send(HTTPStatus.OK, {"elements": elements})

    def log_message(self, format: str, *args: object) -> None:
        print(f"{self.address_string()} {format % args}")

    def _send(self, status: HTTPStatus, body: dict[str, Any]) -> None:
        encoded = json.dumps(body).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", default=18788, type=int)
    parser.add_argument("--threshold", default=0.05, type=float)
    parser.add_argument("--iou-threshold", default=0.45, type=float)
    args = parser.parse_args()

    Handler.detector = OmniParserDetector(args.threshold, args.iou_threshold)
    server = ThreadingHTTPServer((args.host, args.port), Handler)
    print(f"DeviceKit OmniParser sidecar listening on http://{args.host}:{args.port}/locate")
    server.serve_forever()


if __name__ == "__main__":
    main()
