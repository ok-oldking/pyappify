// src/AppIcon.tsx
import {useEffect, useState} from "react";
import {invoke} from "@tauri-apps/api/core";
import {Box} from "@mui/material";

// Mirrors icon_mime_type in src-tauri/src/app_service.rs; keep in sync.
const ICON_MIME_TYPES: Record<string, string> = {
    png: 'image/png',
    jpg: 'image/jpeg',
    jpeg: 'image/jpeg',
    gif: 'image/gif',
    webp: 'image/webp',
    svg: 'image/svg+xml',
    bmp: 'image/bmp',
    ico: 'image/x-icon',
};

const iconMimeType = (iconPath: string): string | undefined =>
    ICON_MIME_TYPES[iconPath.trim().split('.').pop()?.toLowerCase() ?? ''];

export default function AppIcon({appName, iconPath}: { appName: string; iconPath: string }) {
    const [src, setSrc] = useState<string | null>(null);

    useEffect(() => {
        let disposed = false;
        let objectUrl: string | null = null;
        setSrc(null);

        const mimeType = iconPath?.trim() ? iconMimeType(iconPath) : undefined;
        if (!mimeType) return;

        // The backend returns raw bytes (an ArrayBuffer); empty means no usable icon.
        invoke<ArrayBuffer>('get_app_icon', {appName})
            .then((bytes) => {
                if (disposed || bytes.byteLength === 0) return;
                objectUrl = URL.createObjectURL(new Blob([bytes], {type: mimeType}));
                setSrc(objectUrl);
            })
            .catch(() => {
                if (!disposed) setSrc(null);
            });

        return () => {
            disposed = true;
            if (objectUrl) URL.revokeObjectURL(objectUrl);
        };
    }, [appName, iconPath]);

    if (!src) return null;

    return (
        <Box
            component="img"
            src={src}
            alt=""
            sx={{width: 46, height: 46, objectFit: 'contain', flexShrink: 0}}
        />
    );
}
