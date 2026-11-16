// Managed provider contract tests; compiled together with msaa_helper.cs.
public sealed class MsaaTestProvider : Accessibility.IAccessible {
  public int Role = 43, State;
  public string Name = "Save", Value = "", Description;
  public bool FailState;
  public int NameReads;
  public object[] Children = new object[0];
  public object get_accChild(object id) { return null; }
  public int accChildCount { get { return Children.Length; } }
  public object accParent { get { return null; } }
  public object accFocus { get { return null; } }
  public object accSelection { get { return null; } }
  public string get_accName(object id) { NameReads++; return (int)id == 0 ? Name : "Simple child"; }
  public string get_accValue(object id) { return Value; }
  public object get_accRole(object id) { return (int)id == 0 ? Role : 42; }
  public object get_accState(object id) {
    if (FailState) throw new System.Runtime.InteropServices.COMException("unavailable");
    return State;
  }
  public string get_accDescription(object id) { return Description; }
  public string get_accDefaultAction(object id) { return null; }
  public string get_accHelp(object id) { return null; }
  public int get_accHelpTopic(out string file, object id) { file = null; return 0; }
  public string get_accKeyboardShortcut(object id) { return null; }
  public void set_accName(object id, string name) {}
  public void set_accValue(object id, string value) {}
  public void accDoDefaultAction(object id) { throw new System.Exception("inspection must not invoke actions"); }
  public void accSelect(int flags, object id) {}
  public object accNavigate(int direction, object id) { return null; }
  public object accHitTest(int x, int y) { return null; }
  public void accLocation(out int x, out int y, out int w, out int h, object id) { x = -120; y = 20; w = 100; h = 30; }
}

public static class MsaaProviderTests {
  private static object[] Enumerate(Accessibility.IAccessible acc, int start, int count) {
    object[] all = ((MsaaTestProvider)acc).Children;
    int size = System.Math.Min(count, all.Length - start);
    object[] result = new object[size];
    System.Array.Copy(all, start, result, 0, size);
    return result;
  }
  private static DeviceBotMsaa.Reader Read(MsaaTestProvider provider, string filter, int max, int budget, int depth) {
    DeviceBotMsaa.Reader reader = new DeviceBotMsaa.Reader(filter, max, budget, depth, Enumerate);
    reader.Visit(provider, 0, 0);
    return reader;
  }
  private static void Check(bool condition, string message) { if (!condition) throw new System.Exception(message); }
  public static void Run() {
    MsaaTestProvider root = new MsaaTestProvider();
    MsaaTestProvider box = new MsaaTestProvider { Role = 44, State = 0x10 | 4 | 1 };
    root.Children = new object[] { 7, box, root }; // child ID, object, cycle
    var nodes = Read(root, "", 20, 100, 10).Nodes;
    Check(nodes.Count == 3, "simple children, object children and cycle deduplication");
    Check(nodes[1].text == "Simple child" && nodes[1].className == "textField", "child ID metadata");
    Check(nodes[2].@checked == true && nodes[2].focused == true && !nodes[2].enabled, "state mapping");
    Check(nodes[0].left == -120 && nodes[0].right == -20 && nodes[0].value == "", "negative coordinates and empty value");
    Check(nodes[0].resourceId == null, "MSAA must not invent stable resource IDs");
    Check(Read(root, "sImPlE", 20, 100, 10).Nodes.Count == 1, "case insensitive filter traverses unmatched parent");
    Check(Read(root, "", 1, 100, 10).Nodes.Count == 1, "max results");
    Check(Read(root, "", 20, 2, 10).Nodes.Count == 2, "visit budget");
    Check(Read(root, "", 20, 100, 0).Nodes.Count == 1, "depth budget");
    Check(Read(root, "missing", 20, 2, 10).Full, "unmatched nodes consume visit budget");
    box.State = 0x20;
    Check(Read(box, "", 10, 10, 10).Nodes[0].@checked == null, "mixed state is unknown");
    box.State = 0x200002;
    Check(Read(box, "", 10, 10, 10).Nodes[0].selected == true, "selectable state");
    box.State = 0x8000;
    Check(Read(box, "", 10, 10, 10).Nodes.Count == 0, "hidden nodes excluded");
    box.State = 0x10000;
    Check(Read(box, "", 10, 10, 10).Nodes.Count == 0, "offscreen nodes excluded");
    box.State = 0x20000000; box.NameReads = 0;
    var password = Read(box, "", 10, 10, 10).Nodes[0];
    Check(password.value == null && password.text == null && box.NameReads == 0, "protected text never queried");
    box.FailState = true;
    var unknown = Read(box, "", 10, 10, 10).Nodes[0];
    Check(unknown.focused == null && unknown.value == null && !unknown.enabled, "unknown state must not leak values");
    System.Console.WriteLine("MSAA provider contracts passed");
  }
  public static void Main() { Run(); }
}
