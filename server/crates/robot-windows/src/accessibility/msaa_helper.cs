// Compiled by Windows PowerShell 5.1: keep this compatible with C# 5.
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using Accessibility;

public static class DeviceBotMsaa {
  private delegate bool EnumWindow(IntPtr hwnd, IntPtr data);
  [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindow callback, IntPtr data);
  [DllImport("user32.dll")] private static extern bool EnumChildWindows(IntPtr parent, EnumWindow callback, IntPtr data);
  [DllImport("user32.dll")] private static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll")] private static extern bool IsWindow(IntPtr hwnd);
  [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")] private static extern bool IsIconic(IntPtr hwnd);
  [DllImport("user32.dll")] private static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
  [DllImport("oleacc.dll")] private static extern int AccessibleObjectFromWindow(IntPtr hwnd, uint objectId, ref Guid iid,
    [MarshalAs(UnmanagedType.Interface)] out IAccessible accessible);
  [DllImport("oleacc.dll")] private static extern int AccessibleChildren(IAccessible accessible, int start, int count,
    [Out, MarshalAs(UnmanagedType.LPArray, SizeParamIndex = 2)] object[] children, out int obtained);

  public sealed class Node {
    public string className, resourceId, text, contentDescription, value;
    public int left, top, right, bottom;
    public bool clickable, enabled;
    public bool? focused, @checked, selected;
  }

  internal static string Text(string value) { return String.IsNullOrWhiteSpace(value) ? null : value; }
  internal static string Role(int role) {
    switch (role) {
      case 9: return "window";
      case 10: return "client";
      case 11: return "menu";
      case 12: return "menuItem";
      case 20: return "group";
      case 30: return "link";
      case 33: case 35: return "list";
      case 34: case 36: case 37: return "listItem";
      case 40: return "image";
      case 41: return "text";
      case 42: return "textField";
      case 43: case 56: case 57: return "button";
      case 44: return "checkbox";
      case 45: return "radio";
      case 46: return "combobox";
      case 48: return "progressBar";
      case 51: return "slider";
      case 3: return "scrollView";
      default: return "unknown";
    }
  }

  internal sealed class Reader {
    internal readonly List<Node> Nodes = new List<Node>();
    private readonly HashSet<string> seen = new HashSet<string>();
    private readonly int max, budget, maxDepth;
    private readonly Func<IAccessible, int, int, object[]> enumerate;
    private int visited;
    internal bool Full { get { return visited >= budget || Nodes.Count >= max; } }
    internal Reader(int max, int budget, int maxDepth)
      : this(max, budget, maxDepth, Children) {}
    internal Reader(int max, int budget, int maxDepth, Func<IAccessible, int, int, object[]> enumerate) {
      this.max = max;
      this.budget = budget; this.maxDepth = maxDepth;
      this.enumerate = enumerate;
    }
    private static object[] Children(IAccessible acc, int start, int size) {
      object[] children = new object[size]; int obtained;
      int result = AccessibleChildren(acc, start, size, children, out obtained);
      if (result < 0 || obtained <= 0 || obtained > size) return new object[0];
      Array.Resize(ref children, obtained);
      return children;
    }
    internal void Visit(IAccessible acc, int childId, int depth) {
      if (acc == null || Full || depth > maxDepth) return;
      // Count visits so a tree without usable nodes is still bounded.
      visited++;
      IntPtr identity = Marshal.GetIUnknownForObject(acc);
      string key;
      try { key = identity.ToInt64().ToString() + ":" + childId; }
      finally { Marshal.Release(identity); }
      if (!seen.Add(key)) return;
      int? state = null;
      try { state = acc.get_accState(childId) as int?; } catch (COMException) {} catch (ArgumentException) {}
      // INVISIBLE or OFFSCREEN. Do not expose hidden/password content.
      if (state.HasValue && (state.Value & 0x18000) != 0) return;
      Node node = new Node();
      int role = 0;
      // Custom MSAA roles may be strings, not ROLE_SYSTEM integer constants.
      try { role = (acc.get_accRole(childId) as int?) ?? 0; } catch (COMException) {} catch (ArgumentException) {}
      node.className = Role(role);
      bool protectedValue = !state.HasValue || (state.Value & 0x20000000) != 0;
      if (!protectedValue) {
        try { node.text = Text(acc.get_accName(childId)); } catch (COMException) {} catch (ArgumentException) {}
        try { node.value = acc.get_accValue(childId); } catch (COMException) {} catch (ArgumentException) {}
        try { node.contentDescription = Text(acc.get_accDescription(childId)); } catch (COMException) {} catch (ArgumentException) {}
      }
      if (node.contentDescription == null) node.contentDescription = node.text;
      node.enabled = state.HasValue && (state.Value & 1) == 0;
      if (state.HasValue) {
        node.focused = (state.Value & 4) != 0;
        if (role == 44 || role == 45) {
          // MIXED is indeterminate, not false.
          if ((state.Value & 0x20) == 0) node.@checked = (state.Value & 0x10) != 0;
        }
        if ((state.Value & 0x200000) != 0) node.selected = (state.Value & 2) != 0;
      }
      node.clickable = role == 43 || role == 44 || role == 45 || role == 30 || role == 12 || role == 56 || role == 57;
      try { node.clickable |= !String.IsNullOrEmpty(acc.get_accDefaultAction(childId)); } catch (COMException) {} catch (ArgumentException) {}
      try {
        int x, y, width, height;
        acc.accLocation(out x, out y, out width, out height, childId);
        node.left = x; node.top = y;
        node.right = checked(x + width); node.bottom = checked(y + height);
        if (width > 0 && height > 0) Nodes.Add(node);
      } catch (COMException) {} catch (ArgumentException) {} catch (OverflowException) {}
      if (Full || childId != 0 || depth == maxDepth) return;
      int count;
      try { count = acc.accChildCount; } catch (COMException) { return; } catch (ArgumentException) { return; }
      // Enumerate in small batches so a corrupt provider cannot force huge allocation.
      for (int start = 0; start < count && !Full; ) {
        int size = Math.Min(64, Math.Min(count - start, budget - visited));
        object[] children;
        try { children = enumerate(acc, start, size); } catch (COMException) { break; }
        if (children.Length == 0 || children.Length > size) break;
        foreach (object child in children) {
          if (Full) break;
          IAccessible childObject = child as IAccessible;
          if (childObject != null) Visit(childObject, 0, depth + 1);
          else if (child is int && (int)child > 0) {
            // A child ID can identify either a simple element or a full object.
            IAccessible resolved = null;
            try { resolved = acc.get_accChild(child) as IAccessible; } catch (COMException) {} catch (ArgumentException) {}
            if (resolved != null) Visit(resolved, 0, depth + 1);
            else Visit(acc, (int)child, depth + 1);
          }
        }
        start += children.Length;
      }
    }
    internal void Window(IntPtr hwnd, uint pid) {
      uint owner;
      if (Full || !IsWindowVisible(hwnd) || GetWindowThreadProcessId(hwnd, out owner) == 0 || owner != pid) return;
      Guid iid = new Guid("618736e0-3c3d-11cf-810c-00aa00389b71");
      IAccessible acc;
      // OBJID_CLIENT requests the app's semantic provider, not just window chrome.
      int result = AccessibleObjectFromWindow(hwnd, 0xFFFFFFFC, ref iid, out acc);
      if (result == unchecked((int)0x80070005))
        throw new UnauthorizedAccessException("MSAA access denied. Check target/helper elevation and desktop session.");
      if (result >= 0 && acc != null) Visit(acc, 0, 0);
    }
  }

  public static Node[] Read(uint pid, long window, int max, int budget, int depth) {
    if (max <= 0) return new Node[0];
    IntPtr previous = IntPtr.Zero;
    try {
      // Match physical desktop pixels used by native input, including mixed DPI.
      try { previous = SetThreadDpiAwarenessContext(new IntPtr(-4)); } catch (EntryPointNotFoundException) {}
      IntPtr hwnd = new IntPtr(window);
      if (hwnd == IntPtr.Zero && pid == 0) hwnd = GetForegroundWindow();
      uint owner;
      if (hwnd != IntPtr.Zero) {
        if (!IsWindow(hwnd) || GetWindowThreadProcessId(hwnd, out owner) == 0 || (pid != 0 && owner != pid))
          throw new InvalidOperationException("The selected window no longer belongs to the selected app.");
        pid = owner;
      } else {
        EnumWindows(delegate(IntPtr h, IntPtr unused) {
          uint p;
          if (IsWindowVisible(h) && !IsIconic(h) && GetWindowThreadProcessId(h, out p) != 0 && p == pid) { hwnd = h; return false; }
          return true;
        }, IntPtr.Zero);
      }
      if (hwnd == IntPtr.Zero || !IsWindowVisible(hwnd) || IsIconic(hwnd))
        throw new InvalidOperationException("MSAA requires a visible, restored window in the interactive desktop session.");
      Reader reader = new Reader(Math.Min(max, budget), budget, depth);
      reader.Window(hwnd, pid);
      // Flutter's provider may live on its child HWND instead of the runner HWND.
      EnumChildWindows(hwnd, delegate(IntPtr h, IntPtr unused) { reader.Window(h, pid); return !reader.Full; }, IntPtr.Zero);
      return reader.Nodes.ToArray();
    } finally {
      if (previous != IntPtr.Zero) SetThreadDpiAwarenessContext(previous);
    }
  }
}
