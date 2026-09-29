package com.nl2sh.bridge;

import android.accessibilityservice.AccessibilityService;
import android.accessibilityservice.GestureDescription;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.graphics.Path;
import android.graphics.Rect;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.Base64;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityNodeInfo;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayDeque;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicReference;
import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/** Minimal device-local Accessibility adapter. All remote approval stays in nl2sh. */
public final class BridgeService extends AccessibilityService {
    private static final int MAX_NODES = 250;
    private static final int MAX_SNAPSHOT_NODE_BYTES = 60 * 1024;
    private static volatile BridgeService current;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final AtomicBoolean gestureActive = new AtomicBoolean(false);

    @Override
    protected void onServiceConnected() {
        super.onServiceConnected();
        current = this;
    }

    @Override
    public void onAccessibilityEvent(AccessibilityEvent event) { }

    @Override
    public void onInterrupt() { }

    @Override
    public void onDestroy() {
        if (current == this) {
            current = null;
        }
        super.onDestroy();
    }

    static JSONObject dispatch(JSONObject request) {
        BridgeService service = current;
        return service == null ? error("Accessibility service is disabled")
                : service.executeOnMain(request);
    }

    private JSONObject executeOnMain(JSONObject request) {
        if ("gesture".equals(request.optString("action", ""))) {
            return executeGesture(request);
        }
        AtomicReference<JSONObject> reply = new AtomicReference<>();
        AtomicBoolean cancelled = new AtomicBoolean(false);
        CountDownLatch done = new CountDownLatch(1);
        if (!main.post(() -> {
            try {
                if (!cancelled.get()) {
                    reply.set(execute(request, cancelled));
                }
            } catch (Exception failure) {
                reply.set(error("Accessibility action failed"));
            } finally {
                done.countDown();
            }
        })) {
            return error("Accessibility main thread is unavailable");
        }
        try {
            if (!done.await(5, TimeUnit.SECONDS)) {
                cancelled.set(true);
                return error("Accessibility action timed out");
            }
        } catch (InterruptedException interrupted) {
            cancelled.set(true);
            Thread.currentThread().interrupt();
            return error("Accessibility action interrupted");
        }
        JSONObject value = reply.get();
        return value == null ? error("Accessibility action returned no result") : value;
    }

    private JSONObject executeGesture(JSONObject request) {
        final int x;
        final int y;
        final int endX;
        final int endY;
        final int duration;
        try {
            x = Integer.parseInt(request.optString("x", ""));
            y = Integer.parseInt(request.optString("y", ""));
            endX = Integer.parseInt(request.optString("end_x", ""));
            endY = Integer.parseInt(request.optString("end_y", ""));
            duration = Integer.parseInt(request.optString("duration_ms", ""));
        } catch (NumberFormatException invalid) {
            return error("invalid Accessibility gesture parameters");
        }
        int width = getResources().getDisplayMetrics().widthPixels;
        int height = getResources().getDisplayMetrics().heightPixels;
        if (x < 0 || y < 0 || endX < 0 || endY < 0
                || x >= width || endX >= width || y >= height || endY >= height
                || duration < 100 || duration > 10000) {
            return error("Accessibility gesture is outside the current display");
        }
        if (!gestureActive.compareAndSet(false, true)) {
            return error("another Accessibility gesture is active");
        }
        CountDownLatch done = new CountDownLatch(1);
        AtomicReference<JSONObject> reply = new AtomicReference<>();
        AtomicBoolean expired = new AtomicBoolean(false);
        if (!main.post(() -> {
            if (expired.get()) {
                gestureActive.set(false);
                done.countDown();
                return;
            }
            try {
                Path path = new Path();
                path.moveTo(x, y);
                path.lineTo(endX, endY);
                GestureDescription gesture = new GestureDescription.Builder()
                        .addStroke(new GestureDescription.StrokeDescription(path, 0, duration))
                        .build();
                boolean accepted = dispatchGesture(gesture,
                        new GestureResultCallback() {
                            @Override
                            public void onCompleted(GestureDescription completed) {
                                try {
                                    reply.set(success().put("status", "complete")
                                            .put("backend", "accessibility"));
                                } catch (JSONException failure) {
                                    reply.set(error("cannot encode Accessibility gesture result"));
                                } finally {
                                    gestureActive.set(false);
                                    done.countDown();
                                }
                            }

                            @Override
                            public void onCancelled(GestureDescription cancelled) {
                                reply.set(error("Accessibility gesture was cancelled"));
                                gestureActive.set(false);
                                done.countDown();
                            }
                        }, main);
                if (!accepted) {
                    reply.set(error("Accessibility gesture was not accepted"));
                    gestureActive.set(false);
                    done.countDown();
                }
            } catch (RuntimeException failure) {
                reply.set(error("Accessibility gesture failed"));
                gestureActive.set(false);
                done.countDown();
            }
        })) {
            gestureActive.set(false);
            return error("Accessibility main thread is unavailable");
        }
        try {
            if (!done.await(duration + 2000L, TimeUnit.MILLISECONDS)) {
                expired.set(true);
                gestureActive.set(false);
                return error("Accessibility gesture outcome is unknown after timeout");
            }
        } catch (InterruptedException interrupted) {
            expired.set(true);
            gestureActive.set(false);
            Thread.currentThread().interrupt();
            return error("Accessibility gesture outcome is unknown after interruption");
        }
        JSONObject value = reply.get();
        return value == null ? error("Accessibility gesture returned no result") : value;
    }

    private JSONObject execute(JSONObject request, AtomicBoolean cancelled) throws JSONException {
        String action = request.optString("action", "");
        if ("ping".equals(action)) {
            return success().put("status", "ready");
        }
        AccessibilityNodeInfo root = getRootInActiveWindow();
        if (root == null) {
            return error("active window is unavailable");
        }
        if ("screen_dump".equals(action)) {
            return snapshot(root);
        }
        if ("focused_input".equals(action)) {
            AccessibilityNodeInfo focus = focusedEditable(root);
            if (focus == null) {
                return error("focused editable non-password node is unavailable");
            }
            Rect rect = new Rect();
            focus.getBoundsInScreen(rect);
            return success().put("status", "ready")
                    .put("package", nonNull(focus.getPackageName()))
                    .put("class", nonNull(focus.getClassName()))
                    .put("resource_id", nonNull(focus.getViewIdResourceName()))
                    .put("bounds", boundsText(rect));
        }
        if ("focused_target".equals(action)) {
            // Same identity as focused_input but without the isEditable requirement: apps such as
            // Toutiao hide that flag on their search box while still accepting pasted text.
            return focusedTarget(root);
        }
        if ("input_text".equals(action)) {
            return inputText(root, request.optString("text", ""),
                    request.optString("package", ""),
                    request.optString("class", ""), request.optString("resource_id", ""),
                    request.optString("bounds", ""), cancelled);
        }
        if ("tap_text".equals(action)) {
            return clickNode(root, request.optString("text", ""), true,
                    request.optString("bounds", ""), request.optString("package", ""),
                    request.optString("class", ""),
                    request.optString("resource_id", ""),
                    request.optString("node_text", ""),
                    request.optString("description", ""),
                    request.optString("text_hash", ""),
                    request.optString("description_hash", ""), cancelled);
        }
        if ("tap_node".equals(action)) {
            return clickNode(root, "", false,
                    request.optString("bounds", ""), request.optString("package", ""),
                    request.optString("class", ""),
                    request.optString("resource_id", ""),
                    request.optString("node_text", ""),
                    request.optString("description", ""),
                    request.optString("text_hash", ""),
                    request.optString("description_hash", ""), cancelled);
        }
        if ("paste_text".equals(action)) {
            return pasteText(root, request.optString("text", ""),
                    request.optString("package", ""), request.optString("class", ""),
                    request.optString("resource_id", ""), request.optString("bounds", ""),
                    request.optString("node_text", ""), request.optString("description", ""),
                    request.optString("text_hash", ""), request.optString("description_hash", ""),
                    cancelled);
        }
        return error("unsupported action");
    }

    private JSONObject inputText(AccessibilityNodeInfo root, String text, String packageName,
            String className,
            String resourceId, String bounds, AtomicBoolean cancelled) throws JSONException {
        if (text.isEmpty() || text.getBytes(StandardCharsets.UTF_8).length > 1024) {
            return error("text must contain 1–1024 UTF-8 bytes");
        }
        AccessibilityNodeInfo focus = focusedEditable(root);
        if (focus == null) {
            return error("focused editable non-password node is unavailable");
        }
        Rect rect = new Rect();
        focus.getBoundsInScreen(rect);
        if (packageName.isEmpty() || !packageName.equals(nonNull(focus.getPackageName()))
                || !className.equals(nonNull(focus.getClassName()))
                || !resourceId.equals(nonNull(focus.getViewIdResourceName()))
                || !bounds.equals(boundsText(rect))) {
            return error("focused Android UI target changed after confirmation");
        }
        CharSequence existing = focus.getText();
        String prefix = existing == null || focus.isShowingHintText() ? "" : existing.toString();
        if (prefix.length() + text.length() > 4096) {
            return error("focused text exceeds size limit");
        }
        Bundle arguments = new Bundle();
        arguments.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE,
                prefix + text);
        if (cancelled.get()) {
            return error("Accessibility request expired");
        }
        if (!focus.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, arguments)) {
            return error("focused control did not accept text");
        }
        return success().put("status", "complete").put("backend", "accessibility");
    }

    private static AccessibilityNodeInfo focusedEditable(AccessibilityNodeInfo root) {
        AccessibilityNodeInfo focus = root.findFocus(AccessibilityNodeInfo.FOCUS_INPUT);
        return focus != null && focus.isEditable() && !focus.isPassword()
                && focus.isVisibleToUser() && focus.isEnabled() ? focus : null;
    }

    /**
     * Input-focused node identity without the isEditable requirement.
     *
     * Used only to locate a text field for {@link #pasteText}: the write still goes through
     * ACTION_PASTE or an editable ancestor, never through an unverified node.
     */
    private static JSONObject focusedTarget(AccessibilityNodeInfo root) throws JSONException {
        AccessibilityNodeInfo focus = root.findFocus(AccessibilityNodeInfo.FOCUS_INPUT);
        if (focus == null || focus.isPassword() || !focus.isVisibleToUser() || !focus.isEnabled()) {
            return error("focused Android UI node is unavailable");
        }
        Rect rect = new Rect();
        focus.getBoundsInScreen(rect);
        return success().put("status", "ready")
                .put("package", nonNull(focus.getPackageName()))
                .put("class", nonNull(focus.getClassName()))
                .put("resource_id", nonNull(focus.getViewIdResourceName()))
                .put("bounds", boundsText(rect))
                .put("editable", focus.isEditable());
    }

    private JSONObject clickNode(AccessibilityNodeInfo root, String text, boolean byText,
            String bounds, String packageName, String className, String resourceId, String nodeText,
            String description, String textHash, String descriptionHash,
            AtomicBoolean cancelled)
            throws JSONException {
        if (byText && (text.isEmpty() || text.length() > 1024)) {
            return error("invalid text target");
        }
        AccessibilityNodeInfo match;
        try {
            match = findNode(root, text, byText, bounds, packageName, className, resourceId,
                    nodeText, description, textHash, descriptionHash);
        } catch (MatchFailure failure) {
            return error(failure.getMessage());
        }
        AccessibilityNodeInfo clickable = match;
        while (clickable != null && !clickable.isClickable()) {
            clickable = clickable.getParent();
        }
        if (cancelled.get()) {
            return error("Accessibility request expired");
        }
        if (clickable == null || !clickable.performAction(AccessibilityNodeInfo.ACTION_CLICK)) {
            return error("Android UI node did not accept click");
        }
        return success().put("status", "complete").put("backend", "accessibility");
    }

    /** Locate one identity-matched visible node; the match rules are shared by click and paste. */
    private static AccessibilityNodeInfo findNode(AccessibilityNodeInfo root, String text,
            boolean byText, String bounds, String packageName, String className, String resourceId,
            String nodeText, String description, String textHash, String descriptionHash)
            throws MatchFailure {
        AccessibilityNodeInfo match = null;
        ArrayDeque<AccessibilityNodeInfo> pending = new ArrayDeque<>();
        pending.add(root);
        int visited = 0;
        while (!pending.isEmpty() && visited++ < MAX_NODES * 4) {
            AccessibilityNodeInfo node = pending.removeFirst();
            Rect rect = new Rect();
            node.getBoundsInScreen(rect);
            boolean sameText = text.contentEquals(nonNull(node.getText()))
                    || text.contentEquals(nonNull(node.getContentDescription()));
            if (node.isVisibleToUser() && node.isEnabled() && (!byText || sameText)
                    && !packageName.isEmpty()
                    && packageName.equals(nonNull(node.getPackageName()))
                    && bounds.equals(boundsText(rect))
                    && className.equals(nonNull(node.getClassName()))
                    && resourceId.equals(nonNull(node.getViewIdResourceName()))
                    && matchesTextIdentity(nodeText, textHash, node.getText())
                    && matchesTextIdentity(description, descriptionHash,
                            node.getContentDescription())) {
                if (match != null) {
                    throw new MatchFailure("Android UI node match is ambiguous");
                }
                match = node;
            }
            for (int index = 0; index < node.getChildCount(); index++) {
                AccessibilityNodeInfo child = node.getChild(index);
                if (child != null) {
                    pending.addLast(child);
                }
            }
        }
        if (!pending.isEmpty()) {
            throw new MatchFailure("Android UI tree exceeds node limit");
        }
        if (match == null) {
            throw new MatchFailure("Android UI node not found");
        }
        return match;
    }

    /**
     * Paste text into one identity-matched node through the system clipboard.
     *
     * Needed because `ACTION_SET_TEXT` (inputText) requires the node to report isEditable and
     * several apps hide that flag, while the node itself still accepts ACTION_PASTE.
     */
    private JSONObject pasteText(AccessibilityNodeInfo root, String text, String packageName,
            String className, String resourceId, String bounds, String nodeText, String description,
            String textHash, String descriptionHash, AtomicBoolean cancelled) throws JSONException {
        if (text.isEmpty() || text.getBytes(StandardCharsets.UTF_8).length > 4096) {
            return error("text must contain 1-4096 UTF-8 bytes");
        }
        AccessibilityNodeInfo match;
        try {
            match = findNode(root, "", false, bounds, packageName, className, resourceId,
                    nodeText, description, textHash, descriptionHash);
        } catch (MatchFailure failure) {
            return error(failure.getMessage());
        }
        ClipboardManager clipboard = (ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        if (clipboard == null) {
            return error("system clipboard is unavailable");
        }
        if (cancelled.get()) {
            return error("Accessibility request expired");
        }
        clipboard.setPrimaryClip(ClipData.newPlainText("nl2sh", text));
        AccessibilityNodeInfo target = match;
        while (target != null && !target.isEditable()) {
            target = target.getParent();
        }
        if (target == null) {
            target = match;
        }
        target.performAction(AccessibilityNodeInfo.ACTION_FOCUS);
        if (target.performAction(AccessibilityNodeInfo.ACTION_PASTE)) {
            return success().put("status", "complete").put("backend", "accessibility-paste");
        }
        Bundle arguments = new Bundle();
        arguments.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text);
        if (target.isEditable() && target.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, arguments)) {
            return success().put("status", "complete").put("backend", "accessibility");
        }
        return error("Android UI node did not accept pasted text");
    }

    /** Identity-match failure carrying a reply-ready message. */
    private static final class MatchFailure extends Exception {
        private static final long serialVersionUID = 1L;

        MatchFailure(String message) {
            super(message);
        }
    }

    private JSONObject snapshot(AccessibilityNodeInfo root) throws JSONException {
        JSONArray nodes = new JSONArray();
        ArrayDeque<AccessibilityNodeInfo> pending = new ArrayDeque<>();
        pending.add(root);
        int visited = 0;
        int nodeBytes = 0;
        boolean budgetTruncated = false;
        while (!pending.isEmpty() && nodes.length() < MAX_NODES && visited++ < MAX_NODES * 4) {
            AccessibilityNodeInfo node = pending.removeFirst();
            if (!node.isVisibleToUser()) {
                continue;
            }
            Rect rect = new Rect();
            node.getBoundsInScreen(rect);
            JSONObject item = new JSONObject();
            item.put("package", nullable(node.getPackageName()));
            item.put("class", nullable(node.getClassName()));
            item.put("resource_id", nullable(node.getViewIdResourceName()));
            CharSequence text = node.getText();
            item.put("text", node.isPassword() ? JSONObject.NULL : nullable(text));
            item.put("content_description", node.isPassword()
                    ? JSONObject.NULL : nullable(node.getContentDescription()));
            item.put("text_hash", node.isPassword() ? JSONObject.NULL : textHash(text));
            item.put("description_hash", node.isPassword() ? JSONObject.NULL
                    : textHash(node.getContentDescription()));
            item.put("bounds", boundsText(rect));
            item.put("clickable", node.isClickable());
            item.put("focused", node.isFocused());
            int itemBytes = item.toString().getBytes(StandardCharsets.UTF_8).length + 1;
            if (nodeBytes + itemBytes > MAX_SNAPSHOT_NODE_BYTES) {
                budgetTruncated = true;
                break;
            }
            nodes.put(item);
            nodeBytes += itemBytes;
            for (int index = 0; index < node.getChildCount(); index++) {
                AccessibilityNodeInfo child = node.getChild(index);
                if (child != null) {
                    pending.addLast(child);
                }
            }
        }
        boolean truncated = budgetTruncated || !pending.isEmpty();
        return success().put("status", "complete").put("backend", "accessibility")
                .put("node_count", nodes.length()).put("truncated", truncated)
                .put("mode", truncated ? "partial" : "full").put("nodes", nodes);
    }

    private static String nonNull(CharSequence value) {
        return value == null ? "" : value.toString();
    }

    private static Object nullable(CharSequence value) {
        if (value == null || value.length() == 0) {
            return JSONObject.NULL;
        }
        return displayedText(value.toString());
    }

    private static boolean matchesTextIdentity(String expected, String digest,
            CharSequence actual) {
        String value = nonNull(actual);
        if (!expected.equals(displayedText(value))) {
            return false;
        }
        // An older native binary omits the digest. Keep short-node compatibility,
        // but never accept a truncated identity without its full-text digest.
        return digest.isEmpty() ? value.length() <= 256 : digest.equals(textHash(value));
    }

    private static String displayedText(String value) {
        if (value.length() <= 256) {
            return value;
        }
        int end = 256;
        if (Character.isHighSurrogate(value.charAt(end - 1))
                && Character.isLowSurrogate(value.charAt(end))) {
            end--;
        }
        return value.substring(0, end);
    }

    private static String textHash(CharSequence value) {
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            String full = nonNull(value);
            for (int index = 0; index < full.length(); index++) {
                char unit = full.charAt(index);
                digest.update((byte) (unit >>> 8));
                digest.update((byte) unit);
            }
            return Base64.encodeToString(digest.digest(),
                    Base64.URL_SAFE | Base64.NO_PADDING | Base64.NO_WRAP);
        } catch (NoSuchAlgorithmException failure) {
            throw new IllegalStateException("SHA-256 is unavailable", failure);
        }
    }

    private static String boundsText(Rect rect) {
        return "[" + rect.left + "," + rect.top + "][" + rect.right + "," + rect.bottom + "]";
    }

    private static JSONObject success() throws JSONException {
        return new JSONObject().put("ok", true);
    }

    static JSONObject error(String message) {
        try {
            return new JSONObject().put("ok", false).put("error", message);
        } catch (JSONException ignored) {
            return new JSONObject();
        }
    }
}
