# Draft: report of F-104 to the ExifTool forum

> Decision 2026-09-30 (`docs/DECISIONS.md` §3, item 3): report upstream. Posting is for Morii (forum account, outward-facing).
> Board: https://exiftool.org/forum/ → "Bug Reports / Feature Requests". Reproduced again on 2026-09-30 with 13.59, the latest version listed on exiftool.org/history.html that day.

---

**Subject:** 13.59: unknown protobuf fields decode differently the second time in the same process (Google HDR+ MakerNote)

Hi Phil,

When ExifTool reads the same Google HDR+ MakerNote twice in one process with `-u`, the second read returns fewer tags than the first. It happens with two identical files in one command, and between `-execute` calls under `-stay_open`, so a program that compares a file before and after a write sees differences that are not in the file.

Reproduction with the image from the distribution (Windows package 13.59, but it looks platform-independent):

```
copy t\images\Google.jpg a.jpg
copy t\images\Google.jpg b.jpg
exiftool -config "" -u -j -G1 a.jpg b.jpg > out.json
```

The two files are byte-identical, but the first record has 1542 tags and the second 1503. The missing ones are all dynamic `Google:HDRPlusMakerNote_…` tags from deeper in the protobuf, for example `HDRPlusMakerNote_14-1-10-2-4` and `HDRPlusMakerNote_9-47-2-3-1-1-21-1`. After the first two reads the output stays the same.

As far as I can tell, `Protobuf.pm` (around lines 164–180 in 13.59) keeps the `IsProtobuf` flag on the dynamically added tag in the global tag table. The flag can change from 1 to 0 on the first pass, so later reads stop recursing into that field.

We now work around it by re-reading both files in the same state when this happens, so there's no urgency on our side. I'm reporting it because other `-stay_open` users that compare reads could run into the same thing.

Thanks for ExifTool.
