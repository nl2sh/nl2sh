package com.nl2sh.bridge;

import android.content.Context;
import android.content.IntentFilter;
import android.inputmethodservice.InputMethodService;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.util.Base64;
import android.view.Gravity;
import android.view.View;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.ExtractedText;
import android.view.inputmethod.ExtractedTextRequest;
import android.view.inputmethod.InputConnection;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.Toast;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;
import org.json.JSONException;
import org.json.JSONObject;

/**
 * Device-local input method that commits text through the focused editor.
 *
 * Like ADBKeyboard it works through the app's {@link InputConnection}, so Unicode text reaches
 * fields that report neither {@code isEditable} nor {@code ACTION_PASTE}. It has no keyboard:
 * the only view is a button that switches back to the user's previous input method, so
 * selecting nl2sh here never leaves the device without a usable keyboard.
 *
 * All writes stay inside the companion: the Binder provider still requires shell/root, and the
 * bound {@code ime_input_text} action re-checks the approved editor identity before committing.
 */
public final class BridgeImeService extends InputMethodService {
    /** Sender permission for the shell broadcast entry point: held by shell and root only. */
    private static final String SHELL_ONLY_PERMISSION = "android.permission.DUMP";
    /** Matches the largest field content an editor should accept in one commit. */
    private static final int MAX_TEXT_BYTES = 4096;
    private static final int TYPE_MASK_CLASS = 0x0000000f;
    private static final int TYPE_CLASS_TEXT = 0x00000001;
    /** {@code EditorInfo.TYPE_NULL}: no declared class, still accepts committed text. */
    private static final int TYPE_CLASS_NONE = 0x00000000;
    private static final int TYPE_MASK_VARIATION = 0x00000ff0;
    private static final int TYPE_TEXT_VARIATION_PASSWORD = 0x00000080;
    private static final int TYPE_TEXT_VARIATION_VISIBLE_PASSWORD = 0x00000090;
    private static final int TYPE_TEXT_VARIATION_WEB_PASSWORD = 0x000000e0;
    private static volatile BridgeImeService current;
    private final Handler main = new Handler(Looper.getMainLooper());
    private BridgeImeReceiver receiver;
    private boolean receiverRegistered;

    @Override
    public void onCreate() {
        super.onCreate();
        current = this;
        registerShellReceiver();
    }

    @Override
    public void onDestroy() {
        if (receiverRegistered) {
            unregisterReceiver(receiver);
            receiverRegistered = false;
        }
        if (current == this) {
            current = null;
        }
        super.onDestroy();
    }

    /**
     * Accept the ADBKeyboard-style shell broadcasts for as long as this input method is loaded.
     *
     * {@code DUMP} is the permission the sender must hold, so only shell and root can deliver
     * these; a context registration also survives the app being backgrounded, which a manifest
     * receiver does not.
     */
    private void registerShellReceiver() {
        IntentFilter filter = new IntentFilter();
        filter.addAction(BridgeImeReceiver.ACTION_TEXT);
        filter.addAction(BridgeImeReceiver.ACTION_TEXT_B64);
        filter.addAction(BridgeImeReceiver.ACTION_CLEAR);
        receiver = new BridgeImeReceiver();
        // The flags overload exists since API 26 and older platforms ignore the flag, treating
        // the receiver as exported; the sender permission above is what gates delivery.
        registerReceiver(receiver, filter, SHELL_ONLY_PERMISSION, null, Context.RECEIVER_EXPORTED);
        receiverRegistered = true;
    }

    @Override
    public View onCreateInputView() {
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.HORIZONTAL);
        layout.setGravity(Gravity.CENTER);
        Button next = new Button(this);
        next.setText(R.string.switch_keyboard);
        next.setOnClickListener((View ignored) -> switchKeyboard());
        layout.addView(next);
        return layout;
    }

    private void switchKeyboard() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            switchToNextInputMethod(false);
        } else {
            Toast.makeText(this, R.string.switch_keyboard_hint, Toast.LENGTH_LONG).show();
        }
    }

    /**
     * Run one IME action.
     *
     * @param request       action plus optional {@code text}, {@code text_b64}, {@code mode},
     *                      {@code package} and {@code field_id}
     * @param boundEditor   require {@code package}/{@code field_id} to match the current editor
     *                      before writing; true for the Binder provider, false for the
     *                      shell/root broadcast entry point
     */
    static JSONObject dispatch(JSONObject request, boolean boundEditor) {
        BridgeImeService service = current;
        if (service == null) {
            return BridgeService.error("nl2sh keyboard is not loaded; select it in the system"
                    + " input method settings");
        }
        // A broadcast receiver already runs here; posting and waiting would deadlock the main
        // thread, so only cross-thread callers go through the latch.
        if (Looper.myLooper() == service.main.getLooper()) {
            try {
                return service.execute(request, boundEditor);
            } catch (Exception failure) {
                return BridgeService.error("Android input method action failed");
            }
        }
        return service.dispatchFromCallerThread(request, boundEditor);
    }

    private JSONObject dispatchFromCallerThread(JSONObject request, boolean boundEditor) {
        AtomicReference<JSONObject> reply = new AtomicReference<>();
        CountDownLatch done = new CountDownLatch(1);
        if (!main.post(() -> {
            try {
                reply.set(execute(request, boundEditor));
            } catch (Exception failure) {
                reply.set(BridgeService.error("Android input method action failed"));
            } finally {
                done.countDown();
            }
        })) {
            return BridgeService.error("Android input method main thread is unavailable");
        }
        try {
            if (!done.await(5, TimeUnit.SECONDS)) {
                return BridgeService.error("Android input method action timed out");
            }
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
            return BridgeService.error("Android input method action interrupted");
        }
        JSONObject value = reply.get();
        return value == null
                ? BridgeService.error("Android input method returned no result")
                : value;
    }

    private JSONObject execute(JSONObject request, boolean boundEditor) throws JSONException {
        String action = request.optString("action", "");
        if ("ime_status".equals(action)) {
            return status();
        }
        if ("ime_input_text".equals(action)) {
            return commitText(request, boundEditor);
        }
        return BridgeService.error("unsupported action");
    }

    /** Read-only probe: whether the input method is loaded and which editor holds the focus. */
    private JSONObject status() throws JSONException {
        EditorInfo editor = getCurrentInputEditorInfo();
        return BridgeService.success()
                .put("status", "ready")
                .put("attached", getCurrentInputConnection() != null)
                .put("package", editor == null ? "" : nonNull(editor.packageName))
                .put("field_id", editor == null ? 0 : editor.fieldId)
                .put("text_field", editor != null && isTextField(editor.inputType))
                .put("password", editor != null && isPassword(editor.inputType));
    }

    private JSONObject commitText(JSONObject request, boolean boundEditor) throws JSONException {
        String mode = request.optString("mode", "append");
        if (!"append".equals(mode) && !"replace".equals(mode)) {
            return BridgeService.error("invalid Android input method text mode");
        }
        String text;
        try {
            text = decodeText(request);
        } catch (IllegalArgumentException invalid) {
            return BridgeService.error("invalid Android input method text encoding");
        }
        if (text.getBytes(StandardCharsets.UTF_8).length > MAX_TEXT_BYTES) {
            return BridgeService.error("Android input method text exceeds size limit");
        }
        if (text.isEmpty() && "append".equals(mode)) {
            return BridgeService.error("Android input method text must not be empty");
        }
        InputConnection connection = getCurrentInputConnection();
        if (connection == null) {
            return BridgeService.error("nl2sh keyboard has no focused text field");
        }
        EditorInfo editor = getCurrentInputEditorInfo();
        if (editor == null || nonNull(editor.packageName).isEmpty()) {
            return BridgeService.error("focused Android input target is unavailable");
        }
        if (boundEditor && !editorMatches(request, editor)) {
            return BridgeService.error("Android input target changed after confirmation");
        }
        if (!isTextField(editor.inputType)) {
            return BridgeService.error("focused control is not a text field");
        }
        if (isPassword(editor.inputType)) {
            return BridgeService.error("focused control is a password field");
        }
        if ("replace".equals(mode) && !clear(connection)) {
            return BridgeService.error("focused control did not accept cleared text");
        }
        if (!reportsText(connection)) {
            // A window-level connection (WebView, custom view) accepts commitText and drops it.
            // Refuse to report a write that cannot exist instead of silently doing nothing.
            return BridgeService.error("the focused Android editor exposes no text; focus a text"
                    + " field or use the accessibility companion");
        }
        if (!text.isEmpty() && !connection.commitText(text, 1)) {
            return BridgeService.error("focused control did not accept text");
        }
        return BridgeService.success()
                .put("status", "complete")
                .put("backend", "ime")
                .put("mode", mode);
    }

    /** Whether the current editor connection can report its text content. */
    private static boolean reportsText(InputConnection connection) {
        ExtractedText extracted = connection.getExtractedText(new ExtractedTextRequest(), 0);
        return extracted != null && extracted.text != null;
    }

    /** Require the confirmed editor package and field id to still own the input focus. */
    private static boolean editorMatches(JSONObject request, EditorInfo editor) {
        String expectedPackage = request.optString("package", "");
        String expectedField = request.optString("field_id", "");
        return !expectedPackage.isEmpty()
                && expectedPackage.equals(nonNull(editor.packageName))
                && (expectedField.isEmpty() || expectedField.equals(String.valueOf(editor.fieldId)));
    }

    /**
     * Remove the whole current field content so the next commit replaces it.
     *
     * {@code ExtractedText.selectionStart} is the cursor inside the returned text while
     * {@code startOffset} only marks where the extraction window begins, so the surrounding delete
     * needs both clamped against the extracted length. A windowed extraction can leave text
     * behind; that is verified instead of appending to a half-cleared field.
     */
    private static boolean clear(InputConnection connection) {
        ExtractedText extracted = connection.getExtractedText(new ExtractedTextRequest(), 0);
        if (extracted == null || extracted.text == null) {
            return false;
        }
        int length = extracted.text.length();
        int before = Math.max(0, Math.min(extracted.selectionStart, length));
        if (!connection.deleteSurroundingText(before, length - before)) {
            return false;
        }
        ExtractedText remaining = connection.getExtractedText(new ExtractedTextRequest(), 0);
        return remaining != null && remaining.text != null && remaining.text.length() == 0;
    }

    /**
     * Read the text to commit, base64 when the request carries it.
     *
     * ADBKeyboard sends standard base64 while the native client uses the URL-safe alphabet, so
     * both are accepted instead of making the caller match one encoding.
     */
    private static String decodeText(JSONObject request) {
        String encoded = request.optString("text_b64", "");
        if (encoded.isEmpty()) {
            return request.optString("text", "");
        }
        try {
            return new String(Base64.decode(encoded,
                    Base64.URL_SAFE | Base64.NO_PADDING | Base64.NO_WRAP),
                    StandardCharsets.UTF_8);
        } catch (IllegalArgumentException urlSafeFailed) {
            return new String(Base64.decode(encoded, Base64.DEFAULT), StandardCharsets.UTF_8);
        }
    }

    /**
     * Whether the focused editor may receive committed text.
     *
     * WebView and custom views report {@code TYPE_NULL} (editor class 0) for search boxes that
     * still accept {@code commitText}, so only a declared non-text class is refused. Password
     * variations are refused separately by {@link #isPassword}.
     */
    private static boolean isTextField(int inputType) {
        if (isPassword(inputType)) {
            return false;
        }
        int editorClass = inputType & TYPE_MASK_CLASS;
        return editorClass == TYPE_CLASS_TEXT || editorClass == TYPE_CLASS_NONE;
    }

    private static boolean isPassword(int inputType) {
        int variation = inputType & TYPE_MASK_VARIATION;
        return variation == TYPE_TEXT_VARIATION_PASSWORD
                || variation == TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
                || variation == TYPE_TEXT_VARIATION_WEB_PASSWORD;
    }

    private static String nonNull(CharSequence value) {
        return value == null ? "" : value.toString();
    }
}
